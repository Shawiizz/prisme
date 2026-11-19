import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';
import { StemPart, hasPart, setPartLevel, togglePart } from '../stem-parts';
import { StemMix } from '../models/stems';
import { Channel, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { semitonesToMix, transposeCamelot } from '../../shared/camelot';
import {
  Cue,
  MAX_DECKS,
  DeckTrackUpdate,
  DEFAULT_FX,
  FLAT_STRIP,
  MusicalKey,
  DeckSettings,
  EngineStatus,
  MAX_KEY_SHIFT,
  MixerState,
  Rpm,
  SAMPLE_SLOTS,
  TrackAnalysis,
  TrackInfo,
  VinylMode,
  Waveform,
  addWaveformPiece,
  decodeWaveform,
} from '../models/engine';
import { PreferencesService } from './preferences.service';
import { Playhead, followPlayhead } from '../playhead';
import { ConfirmService } from '../../shared/confirm/confirm';
import { deckLetter } from '../../shared/time';
import { t } from '../i18n/i18n';
import { errorText } from '../errors';

/** A deck this close to its cue point counts as sitting on it. */
const CUE_TOLERANCE_S = 0.02;
const MAIN_CUE_COLOR = 0xffb02e;
/** Speed change while a bend key is held. */
const PITCH_BEND = 0.03;
/** Slower than this, a deck under its record is still: a hand holding it, the record stopped. */
const MOVING_SPEED = 0.05;

const DEFAULT_DECK: DeckSettings = {
  pitch: 1,
  volume: 1,
  cue: false,
  vinylEnabled: false,
  vinylMode: 'absolute',
  rpm: '33',
  leadInS: null,
  swapChannels: false,
  timecode: 'serato_cv02_a',
  keyLock: false,
  faderStart: false,
  pitchRange: null,
  strip: FLAT_STRIP,
  fx: DEFAULT_FX,
  samples: Array(SAMPLE_SLOTS).fill(null),
  samplerGain: 1,
  sizeBeats: 4,
  passthrough: false,
  side: 'left',
  jogScratch: true,
  repeat: false,
  mute: false,
};

/**
 * Bridge to the Rust engine: commands go through `invoke`, the engine state arrives
 * ~60 times per second through a Tauri channel.
 */
@Injectable({ providedIn: 'root' })
export class EngineService {
  private readonly preferences = inject(PreferencesService);
  private readonly confirm = inject(ConfirmService);

  /** Latest engine state (drives the templates). */
  readonly status = signal<EngineStatus | null>(null);
  readonly tracks = signal<(TrackInfo | null)[]>(Array(MAX_DECKS).fill(null));
  readonly loading = signal<boolean[]>(Array(MAX_DECKS).fill(false));
  /** Waveform of each deck's track, once fetched. */
  readonly waveforms = signal<(Waveform | null)[]>(Array(MAX_DECKS).fill(null));
  readonly decks = signal<DeckSettings[]>(
    // A and C on the left of the crossfader, B and D on its right
    Array.from({ length: MAX_DECKS }, (_, d) => ({
      ...DEFAULT_DECK,
      side: d % 2 === 0 ? ('left' as const) : ('right' as const),
    })),
  );
  /** The decks shown and played: two, or four. */
  readonly deckIndexes = computed(() =>
    Array.from(
      { length: this.preferences.dj().deckCount === MAX_DECKS ? MAX_DECKS : 2 },
      (_, i) => i,
    ),
  );
  readonly error = signal<string | null>(null);

  readonly audioRunning = computed(() => this.status()?.audioRunning ?? false);

  constructor() {
    // what failed while the audio was away is behind once it runs again
    effect(() => {
      if (this.audioRunning()) untracked(() => this.error.set(null));
    });
  }

  private readonly previewing = new Set<number>();
  /** Where each deck's playhead was when, and its speed: what it is drawn from. */
  private playheads: (Playhead | null)[] = [];

  /** Subscribes to the engine state; returns the mixer settings for the mixer service. */
  async connect(): Promise<MixerState> {
    const channel = new Channel<EngineStatus>();
    channel.onmessage = (s) => {
      this.playheads = s.decks.map((d, i) => followPlayhead(this.playheads[i] ?? null, d));
      this.status.set(s);
    };
    await invoke('subscribe_status', { channel });
    await listen<DeckTrackUpdate>('deck-track-update', (e) => this.completeTrack(e.payload));
    // a track the Auto DJ put on a deck
    await listen<TrackInfo>('deck-loaded', ({ payload }) => this.show(payload.deck, payload));
    // a tempo set by the backend: a controller's tempo fader, the Auto DJ, a record letting go
    await listen<{ deck: number; ratio: number }>('deck-pitch', ({ payload }) =>
      this.patchDeck(payload.deck, { pitch: payload.ratio }),
    );
    const [tracks, mixer] = await Promise.all([
      invoke<(TrackInfo | null)[]>('loaded_tracks'),
      invoke<MixerState>('mixer_state'),
    ]);
    this.tracks.set(tracks);
    tracks.forEach((t, deck) => t && this.watchWaveform(deck, t.loadId));
    this.decks.set(mixer.decks.map((d) => ({ ...DEFAULT_DECK, ...d })));
    return mixer;
  }

  /** Matches the leader once and leaves the deck free (unlocking it first). */
  async syncOnce(deck: number) {
    if (this.status()?.decks[deck]?.sync.locked) await this.unlock(deck);
    return this.call('sync_once', { deck });
  }

  /**
   * Position of a deck extrapolated to this instant, for smooth 60 fps drawing between
   * two status updates.
   */
  livePosition(deck: number): number | null {
    const playhead = this.playheads[deck];
    if (!playhead) return null;
    const elapsed = Math.min((performance.now() - playhead.at) / 1000, 0.05);
    return playhead.positionS + playhead.speed * elapsed;
  }

  /**
   * Playback rate of a deck: from the engine's tempo when the track has a beat grid
   * (sync and platter included), else the platter speed while it turns, else the tempo
   * fader.
   */
  playbackRate(deck: number): number {
    const tempoRate = this.tempoRate(deck);
    if (tempoRate !== null) return tempoRate;
    const settings = this.decks()[deck];
    const speed = Math.abs(this.status()?.decks[deck]?.speed ?? 0);
    return settings.vinylEnabled && speed > MOVING_SPEED ? speed : settings.pitch;
  }

  /**
   * The key heard on a deck: the track's, moved by the speed unless the key lock holds
   * it. A key shift holds it too (it goes through the same stretcher).
   */
  heardKey(deck: number): MusicalKey | null {
    const track = this.tracks()[deck];
    if (!track?.key) return null;
    const held = this.decks()[deck].keyLock || track.keyShift !== 0;
    const semitones = held ? track.keyShift : Math.round(12 * Math.log2(this.playbackRate(deck)));
    return semitones === 0 ? track.key : transposeCamelot(track.key.camelot, semitones);
  }

  setKeyShift(deck: number, semitones: number) {
    const track = this.tracks()[deck];
    if (!track) return;
    const keyShift = Math.max(-MAX_KEY_SHIFT, Math.min(MAX_KEY_SHIFT, semitones));
    this.patch(this.tracks, deck, { ...track, keyShift });
    return this.call('set_key_shift', { deck, semitones: keyShift });
  }

  /** Reads again what is on the decks (fewer decks leave the others empty). */
  async refreshTracks() {
    const tracks = await invoke<(TrackInfo | null)[]>('loaded_tracks');
    this.tracks.set(tracks);
    this.waveforms.update((waveforms) => waveforms.map((w, deck) => (tracks[deck] ? w : null)));
  }

  /** The deck `deck` is mixed with: another one playing, else the one beside it (A and B, C
   * and D). */
  mixedWith(deck: number): number {
    const playing = (this.status()?.decks ?? []).findIndex((d, i) => i !== deck && d.playing);
    return playing >= 0 ? playing : deck ^ 1;
  }

  /** The smallest key shift after which `deck` mixes with the other deck, if both keys are known. */
  keyMatch(deck: number): number | null {
    const own = this.tracks()[deck]?.key;
    const other = this.heardKey(this.mixedWith(deck));
    return own && other ? semitonesToMix(own.camelot, other.camelot) : null;
  }

  /** Mutes or plays a part of the track on `deck` (until the next load), on the next bar
   * with quantize. */
  toggleStem(deck: number, part: StemPart) {
    const onBar = this.preferences.dj().quantize;
    return this.changeStemMix(deck, part, (mix) => togglePart(mix, part), onBar);
  }

  /** Sets how loud a part of the track on `deck` plays (0 to 1, until the next load). */
  setStemLevel(deck: number, part: StemPart, level: number) {
    return this.changeStemMix(deck, part, (mix) => setPartLevel(mix, part, level), false);
  }

  private changeStemMix(
    deck: number,
    part: StemPart,
    change: (mix: StemMix) => StemMix,
    onBar: boolean,
  ) {
    const track = this.tracks()[deck];
    if (!track || !hasPart(track.separation, part)) return;
    const stemMix = change(track.stemMix);
    this.patch(this.tracks, deck, { ...track, stemMix });
    return this.call('set_stem_mix', { deck, mix: stemMix, onBar });
  }

  setKeyLock(deck: number, enabled: boolean) {
    this.patchDeck(deck, { keyLock: enabled });
    return this.call('set_key_lock', { deck, enabled });
  }

  /** The deck plays its input as it comes (a record, a CD player) instead of its track. */
  setPassthrough(deck: number, enabled: boolean) {
    this.patchDeck(deck, { passthrough: enabled });
    return this.call('set_passthrough', { deck, enabled });
  }

  /** The range of the tempo fader of `deck`: its own, else the general one (0.08 = ±8 %). */
  tempoRange(deck: number): number {
    return this.decks()[deck].pitchRange ?? this.preferences.dj().pitchRange;
  }

  /** The range of the tempo fader of `deck`, or of every deck (`null`), which then all follow
   * the general one. */
  setTempoRange(deck: number | null, range: number) {
    if (deck !== null) {
      this.patchDeck(deck, { pitchRange: range });
    } else {
      this.preferences.dj.update((dj) => ({ ...dj, pitchRange: range }));
      this.decks.update((decks) => decks.map((d) => ({ ...d, pitchRange: null })));
    }
    return this.call('set_tempo_range', { deck, range });
  }

  /** The headphones on `deck` alone. */
  soloCue(deck: number) {
    for (const d of this.deckIndexes()) {
      this.patchDeck(d, { cue: d === deck });
      void this.call('set_deck_cue', { deck: d, enabled: d === deck });
    }
  }

  /** Software mixer: the channel fader starts and stops the deck. */
  setFaderStart(deck: number, enabled: boolean) {
    this.patchDeck(deck, { faderStart: enabled });
    return this.call('set_fader_start', { deck, enabled });
  }

  /** The deck's channel silenced in the master, after its fader, or heard again. */
  setMute(deck: number, enabled: boolean) {
    this.patchDeck(deck, { mute: enabled });
    return this.call('set_mute', { deck, enabled });
  }

  /** At the end, the track goes on from its start. */
  setRepeat(deck: number, enabled: boolean) {
    this.patchDeck(deck, { repeat: enabled });
    return this.call('set_repeat', { deck, enabled });
  }

  /** Playback rate of the tempo the engine reports (sync and platter included), if the track has a beat grid. */
  tempoRate(deck: number): number | null {
    const bpm = this.status()?.decks[deck]?.bpm;
    const trackBpm = this.tracks()[deck]?.tempo?.bpm;
    return bpm && trackBpm ? bpm / trackBpm : null;
  }

  /** Loads `path` on `deck`; whether it was. */
  async load(deck: number, path: string): Promise<boolean> {
    if (!(await this.mayReplace(deck))) return false;
    return this.put(deck, () => invoke<TrackInfo | null>('load_track', { deck, path }));
  }

  /** Takes the track off `deck`; whether it was. */
  async eject(deck: number): Promise<boolean> {
    if (!(await this.mayReplace(deck, 'eject'))) return false;
    await this.call('eject_track', { deck });
    const without = <T>(values: T[]) => values.map((v, d) => (d === deck ? null : v));
    this.tracks.update(without);
    this.waveforms.update(without);
    return true;
  }

  /** Whether a touch of the controller's jog wheel of `deck` holds the record. */
  setJogScratch(deck: number, on: boolean) {
    this.patchDeck(deck, { jogScratch: on });
    return this.call('set_jog_scratch', { deck, on });
  }

  /** Stores the loop of `deck` from `startS` to `endS`, playing it if `active`; without them,
   * forgets it. */
  setLoop(deck: number, startS: number | null, endS: number | null, active: boolean) {
    return this.call('set_loop', { deck, startS, endS, active });
  }

  /** The track of deck `from` on `deck`, where it is and playing if it plays. */
  async double(from: number, deck: number): Promise<void> {
    if (!this.tracks()[from] || !(await this.mayReplace(deck))) return;
    await this.put(deck, () => invoke<TrackInfo | null>('double_deck', { from, deck }));
  }

  /** The track of a deck that plays, by itself or under its record, gives way to another or
   * leaves the deck only if the DJ says so; never, if the DJ chose so. */
  private async mayReplace(deck: number, by: 'load' | 'eject' = 'load'): Promise<boolean> {
    const d = this.status()?.decks[deck];
    if (!d?.loaded || (!d.playing && Math.abs(d.speed) < MOVING_SPEED)) return true;
    const letter = { deck: deckLetter(deck) };
    const eject = by === 'eject';
    if (this.preferences.dj().protectPlaying) {
      this.error.set(
        eject
          ? t('Deck {deck} is playing: its track stays on it', letter)
          : t('Deck {deck} is playing: nothing is loaded on it', letter),
      );
      return false;
    }
    return this.confirm.ask({
      title: t('Deck {deck} is playing', letter),
      detail: eject
        ? t('The track playing stops and leaves the deck.')
        : t('The track playing stops, and the new one takes its place.'),
      confirm: eject ? t('Eject') : t('Load'),
      cancel: t('Cancel'),
    });
  }

  /** Puts on `deck` what `load` gives, nothing if another load asked for there since goes
   * there instead; whether it came. */
  private async put(deck: number, load: () => Promise<TrackInfo | null>): Promise<boolean> {
    this.patch(this.loading, deck, true);
    try {
      const info = await load();
      if (!info) return false;
      this.show(deck, info);
      if (this.preferences.dj().cueOnLoad) this.soloCue(deck);
      this.error.set(null);
      return true;
    } catch (e) {
      this.error.set(errorText(e));
      return false;
    } finally {
      this.patch(this.loading, deck, false);
    }
  }

  /** `info` is on `deck` now: its waveform follows. */
  private show(deck: number, info: TrackInfo) {
    this.patch(this.tracks, deck, info);
    this.patch(this.waveforms, deck, null);
    this.watchWaveform(deck, info.loadId);
  }

  /** PLAY; pressed while CUE previews, the deck plays on once CUE is let go, as on a CDJ. */
  togglePlay(deck: number) {
    if (this.previewing.delete(deck)) return;
    const status = this.status()?.decks[deck];
    if (status?.loaded) this.setPlaying(deck, !status.playing);
  }

  setPlaying(deck: number, playing: boolean) {
    return this.call('set_playing', { deck, playing });
  }

  seek(deck: number, seconds: number) {
    return this.call('seek', { deck, seconds: Math.max(0, seconds) });
  }

  /** The track dragged by hand to `seconds` at `atMs` (the pointer event's time), as the
   * scrolling waveform is moved like a record, or let go (`null`). */
  hand(deck: number, seconds: number | null, atMs = performance.now()) {
    const at = seconds === null ? null : Math.max(0, seconds);
    return this.call('hand', { deck, seconds: at, atMs });
  }

  /** The edge of a jog wheel turned by `turns` (positive forwards) at `atMs`: a playing
   * track pushed or held back for a moment, a stopped one moved. */
  jog(deck: number, turns: number, atMs = performance.now()) {
    return this.call('jog', { deck, turns, atMs });
  }

  setPitch(deck: number, ratio: number) {
    this.patchDeck(deck, { pitch: ratio });
    return this.call('set_pitch', { deck, ratio });
  }

  setVinyl(
    deck: number,
    changes: { enabled?: boolean; mode?: VinylMode; rpm?: Rpm; leadInS?: number | null },
  ) {
    const d = this.patchDeck(deck, {
      vinylEnabled: changes.enabled ?? this.decks()[deck].vinylEnabled,
      vinylMode: changes.mode ?? this.decks()[deck].vinylMode,
      rpm: changes.rpm ?? this.decks()[deck].rpm,
      leadInS: changes.leadInS !== undefined ? changes.leadInS : this.decks()[deck].leadInS,
    });
    return this.takeTurntable(deck, d);
  }

  /** A deck put on a record takes the turntable from the deck layered with it, which goes
   * on by itself. */
  private async takeTurntable(deck: number, d: DeckSettings) {
    try {
      const freed = await invoke<number | null>('set_vinyl', {
        deck,
        enabled: d.vinylEnabled,
        mode: d.vinylMode,
        rpm: d.rpm,
        leadInS: d.leadInS,
      });
      if (freed !== null) this.patchDeck(freed, { vinylEnabled: false });
    } catch (e) {
      this.error.set(errorText(e));
    }
  }

  setVinylSwap(deck: number, swap: boolean) {
    this.patchDeck(deck, { swapChannels: swap });
    return this.call('set_vinyl_swap', { deck, swap });
  }

  /** The record on the deck's turntable (a timecode format's id): on the deck layered with
   * it too, which reads the same turntable. */
  async setTimecode(deck: number, format: string) {
    this.patchDeck(deck, { timecode: format });
    try {
      const decks = await invoke<number[]>('set_timecode', { deck, format });
      for (const d of decks) this.patchDeck(d, { timecode: format });
      this.error.set(null);
    } catch (e) {
      this.error.set(errorText(e));
    }
  }

  /**
   * SYNC button: unlocks a locked deck; otherwise locks it, or on a relative record in
   * one-shot mode matches it once (pressing again re-matches after a drift).
   */
  toggleSync(deck: number) {
    const status = this.status()?.decks[deck];
    if (!status) return;
    return this.setSync(deck, !status.sync.locked);
  }

  /** SYNC on (locked, or matched once on a relative record in one-shot mode), or off. */
  setSync(deck: number, on: boolean) {
    if (!on) return this.unlock(deck);
    const oneShot = this.decks()[deck].vinylEnabled && this.preferences.dj().vinylSync === 'once';
    return oneShot
      ? this.call('sync_once', { deck })
      : this.call('set_sync_locked', { deck, locked: true });
  }

  /** Takes SYNC off. A deck playing by itself keeps the tempo it was locked to: its fader is
   * moved there first (a locked deck does not follow it), and a controller's picks it up. */
  private async unlock(deck: number) {
    const d = this.status()?.decks[deck];
    const rate = this.tempoRate(deck);
    if (d?.sync.locked && !d.vinyl.enabled && rate !== null) await this.setPitch(deck, rate);
    return this.call('set_sync_locked', { deck, locked: false });
  }

  /** Whether the deck sits on its cue point. */
  atCue(deck: number): boolean {
    const d = this.status()?.decks[deck];
    return (
      !!d?.loaded && Math.abs(d.positionS - (this.mainCue(deck)?.positionS ?? 0)) < CUE_TOLERANCE_S
    );
  }

  /**
   * CDJ cue, pressed: while playing, back to the cue point and stop; stopped on the cue
   * point, play while held; stopped elsewhere, set the cue point there.
   */
  async cueDown(deck: number) {
    const d = this.status()?.decks[deck];
    if (!d?.loaded) return;
    const cue = this.mainCue(deck)?.positionS ?? 0;
    if (d.playing) {
      this.returnToCue(deck);
    } else if (Math.abs(d.positionS - cue) < CUE_TOLERANCE_S) {
      this.previewing.add(deck);
      this.setPlaying(deck, true);
    } else {
      await this.setMainCue(deck, d.positionS);
    }
  }

  /** CDJ cue, released: a preview goes back to the cue point. */
  cueUp(deck: number) {
    if (this.previewing.delete(deck)) this.returnToCue(deck);
  }

  /** -1, 0 or 1: slower, released, faster. */
  pitchBend(deck: number, direction: -1 | 0 | 1) {
    return this.call('set_pitch_bend', { deck, amount: direction * PITCH_BEND });
  }

  private returnToCue(deck: number) {
    this.setPlaying(deck, false);
    this.seek(deck, this.mainCue(deck)?.positionS ?? 0);
  }

  /** Stores the cue point (the backend may move it onto the beat) and goes there. */
  async setMainCue(deck: number, seconds: number) {
    try {
      const positionS = await invoke<number>('set_main_cue', { deck, seconds });
      const main: Cue = {
        hot: null,
        positionS,
        loopLengthS: null,
        color: MAIN_CUE_COLOR,
        label: '',
      };
      this.setCues(deck, [
        ...(this.tracks()[deck]?.cues ?? []).filter((c) => c.hot !== null),
        main,
      ]);
      this.seek(deck, positionS);
    } catch (e) {
      this.error.set(errorText(e));
    }
  }

  mainCue(deck: number): Cue | null {
    return this.tracks()[deck]?.cues.find((c) => c.hot === null) ?? null;
  }

  /** Replaces the cues of the track on `deck` (after the backend changed them). */
  setCues(deck: number, cues: Cue[]) {
    const track = this.tracks()[deck];
    if (track) this.patch(this.tracks, deck, { ...track, cues });
  }

  /** Analysis and stems that arrive after the track was put on its deck. */
  private completeTrack({ deck, loadId, analysis, separation }: DeckTrackUpdate) {
    const track = this.tracks()[deck];
    if (track?.loadId !== loadId) return;
    if (analysis || separation) {
      this.patch(this.tracks, deck, {
        ...track,
        ...analysis,
        separation: separation ?? track.separation,
      });
    }
  }

  /** The waveform of the load `loadId` on `deck`: what is known of it at once, then, while the
   * track is read, the rest piece by piece. */
  private watchWaveform(deck: number, loadId: number) {
    const channel = new Channel<ArrayBuffer>();
    channel.onmessage = (buffer) => {
      const piece = decodeWaveform(buffer);
      if (piece && this.tracks()[deck]?.loadId === loadId)
        this.patch(this.waveforms, deck, addWaveformPiece(this.waveforms()[deck], piece));
    };
    invoke('watch_waveform', { deck, loadId, channel }).catch(() => {});
  }

  /** Replaces the tempo and key of the track on `deck` (after a correction). */
  setAnalysis(deck: number, analysis: TrackAnalysis) {
    const track = this.tracks()[deck];
    if (track) this.patch(this.tracks, deck, { ...track, ...analysis });
  }

  patchDeck(deck: number, changes: Partial<DeckSettings>): DeckSettings {
    const next = { ...this.decks()[deck], ...changes };
    this.patch(this.decks, deck, next);
    return next;
  }

  private patch<T>(s: { (): T[]; set(v: T[]): void }, index: number, value: T) {
    const copy = [...s()];
    copy[index] = value;
    s.set(copy);
  }

  /** Runs a backend command; a failure is shown rather than thrown, until one goes through. */
  async call(command: string, args: Record<string, unknown>): Promise<void> {
    try {
      await invoke(command, args);
      this.error.set(null);
    } catch (e) {
      this.error.set(errorText(e));
    }
  }
}
