import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';
import { StemPart, partId } from '../stem-parts';
import { Channel, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { semitonesToMix, transposeCamelot } from '../../shared/camelot';
import {
  ControlState,
  MAX_DECKS,
  DeckTrackUpdate,
  DEFAULT_FX,
  FLAT_STRIP,
  MusicalKey,
  DeckSettings,
  EngineStatus,
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
import { same } from '../same';
import { deckLetter } from '../../shared/time';
import { t } from '../i18n/i18n';
import { errorText, isBackendError } from '../errors';

/** Slower than this, a deck under its record is still: a hand holding it, the record stopped. */
const MOVING_SPEED = 0.05;
/** How long a refusal stays said. */
const REFUSAL_MS = 5000;
const NONE_LOADING: boolean[] = Array(MAX_DECKS).fill(false);

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
  /** What the app mirrors as the backend last told it changed (`EngineStatus.controls`). */
  readonly controls = signal<ControlState | null>(null);
  readonly tracks = signal<(TrackInfo | null)[]>(Array(MAX_DECKS).fill(null));
  /** Whether each deck loads a track. */
  readonly loading = computed(() => this.controls()?.loading ?? NONE_LOADING);
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
  private readonly deckCount = computed(() => this.deckIndexes().length);
  readonly error = signal<string | null>(null);
  /** Why what the DJ just asked was not done, nothing being wrong: said for a few seconds,
   * or until the next thing asked. */
  readonly refusal = signal<string | null>(null);
  private refusalTimer?: ReturnType<typeof setTimeout>;

  readonly audioRunning = computed(() => this.status()?.audioRunning ?? false);

  constructor() {
    // what failed while the audio was away is behind once it runs again
    effect(() => {
      if (this.audioRunning()) untracked(() => this.error.set(null));
    });
    // fewer decks leave the others empty, as the backend leaves them
    effect(() => {
      const count = this.deckCount();
      const kept = <T>(values: (T | null)[]) => values.map((v, d) => (d < count ? v : null));
      untracked(() => {
        this.tracks.update(kept);
        this.waveforms.update(kept);
      });
    });
  }

  /** Where each deck's playhead was when, and its speed: what it is drawn from. */
  private playheads: (Playhead | null)[] = [];

  /** Subscribes to the engine state; returns the mixer settings for the mixer service. */
  async connect(): Promise<MixerState> {
    const channel = new Channel<EngineStatus>();
    channel.onmessage = (s) => {
      this.playheads = s.decks.map((d, i) => followPlayhead(this.playheads[i] ?? null, d));
      if (s.controls) this.applyControls(s.controls);
      this.status.set(s);
    };
    await invoke('subscribe_status', { channel });
    await listen<DeckTrackUpdate>('deck-track-update', (e) => this.completeTrack(e.payload));
    const mixer = await invoke<MixerState>('mixer_state');
    this.showDecks(mixer);
    return mixer;
  }

  /** The decks' settings as the backend has them once the audio starts again, which may
   * change them: the simulated turntable taking deck A as it comes, decks layered on one
   * turntable given its record. */
  async reloadDecks() {
    this.showDecks(await invoke<MixerState>('mixer_state'));
  }

  private showDecks(mixer: MixerState) {
    this.decks.set(mixer.decks.map((d) => ({ ...DEFAULT_DECK, ...d })));
  }

  /** What the backend changed of what the app mirrors: the decks' settings, their tracks (one
   * gone leaves its deck, one not shown yet is asked for) with their key shifts, stem mixes and
   * cues, the DJ's quantize and tempo range. What is the same stays as it is, for the views
   * that follow it. */
  private applyControls(controls: ControlState) {
    const decks = controls.mixer.decks.map((d, deck) => {
      const next = { ...DEFAULT_DECK, ...d };
      return same(next, this.decks()[deck]) ? this.decks()[deck] : next;
    });
    if (decks.some((d, deck) => d !== this.decks()[deck])) this.decks.set(decks);
    const tracks = this.tracks().map((track, deck) => {
      const told = controls.tracks[deck];
      if (!told) return null;
      if (track?.loadId !== told.loadId) return track;
      const { keyShift, stemMix, cues } = told;
      const next = { ...track, keyShift, stemMix, cues };
      return same(next, track) ? track : next;
    });
    if (tracks.some((t, deck) => t !== this.tracks()[deck])) {
      this.tracks.set(tracks);
      this.waveforms.update((waveforms) => waveforms.map((w, deck) => (tracks[deck] ? w : null)));
    }
    const { quantize, pitchRange } = controls;
    const dj = this.preferences.dj();
    if (dj.quantize !== quantize || dj.pitchRange !== pitchRange)
      this.preferences.dj.set({ ...dj, quantize, pitchRange });
    this.controls.set(controls);
    if (this.unknownTrack()) void this.fetchTracks();
  }

  /** Whether the backend has a track on a deck that the app does not show yet. */
  private unknownTrack(): boolean {
    const told = this.controls()?.tracks ?? [];
    return told.some((t, deck) => t !== null && t.loadId !== this.tracks()[deck]?.loadId);
  }

  private fetching = false;

  /** Shows the tracks the backend put on the decks, whoever loaded them (the DJ, a controller,
   * the Auto DJ), asking again while it tells of others meanwhile. */
  private async fetchTracks() {
    if (this.fetching) return;
    this.fetching = true;
    try {
      let asked: ControlState | null;
      do {
        asked = this.controls();
        const tracks = await invoke<(TrackInfo | null)[]>('loaded_tracks');
        tracks.forEach((info, deck) => {
          const told = this.controls()?.tracks[deck];
          if (info && info.loadId === told?.loadId && info.loadId !== this.tracks()[deck]?.loadId)
            this.show(deck, info);
        });
      } while (this.controls() !== asked && this.unknownTrack());
    } catch (e) {
      this.error.set(errorText(e));
    } finally {
      this.fetching = false;
    }
  }

  /** Matches the leader once and leaves the deck free (unlocking it first). */
  syncOnce(deck: number) {
    return this.perform(`deck${deck + 1}.sync.once`, 1);
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

  /** The key of the track on `deck` moved by `semitones`, within an octave either way. */
  setKeyShift(deck: number, semitones: number) {
    return this.press(deck, `key.${semitones}`);
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
    return this.press(deck, partId(part));
  }

  /** Sets how loud a part of the track on `deck` plays (0 to 1, until the next load). */
  setStemLevel(deck: number, part: StemPart, level: number) {
    return this.perform(`deck${deck + 1}.${partId(part)}.level`, level);
  }

  setKeyLock(deck: number, enabled: boolean) {
    return this.turn(`deck${deck + 1}.keylock`, enabled);
  }

  /** The deck plays its input as it comes (a record, a CD player) instead of its track. */
  setPassthrough(deck: number, enabled: boolean) {
    return this.turn(`deck${deck + 1}.passthrough`, enabled);
  }

  /** The range of the tempo fader of `deck`: its own, else the general one (0.08 = ±8 %). */
  tempoRange(deck: number): number {
    return this.decks()[deck].pitchRange ?? this.preferences.dj().pitchRange;
  }

  /** The range of the tempo fader of `deck`, or of every deck (`null`), which then all follow
   * the general one. */
  setTempoRange(deck: number | null, range: number) {
    return this.call('set_tempo_range', { deck, range });
  }

  /** The headphones on `deck` alone. */
  soloCue(deck: number) {
    return this.perform(`deck${deck + 1}.headphones.solo`, 1);
  }

  /** Software mixer: the channel fader starts and stops the deck. */
  setFaderStart(deck: number, enabled: boolean) {
    return this.turn(`deck${deck + 1}.faderStart`, enabled);
  }

  /** The deck's channel silenced in the master, after its fader, or heard again. */
  setMute(deck: number, enabled: boolean) {
    return this.turn(`deck${deck + 1}.mute`, enabled);
  }

  /** At the end, the track goes on from its start. */
  setRepeat(deck: number, enabled: boolean) {
    return this.turn(`deck${deck + 1}.repeat`, enabled);
  }

  /** Playback rate of the tempo the engine reports (sync and platter included), if the track has a beat grid. */
  tempoRate(deck: number): number | null {
    const bpm = this.status()?.decks[deck]?.bpm;
    const trackBpm = this.tracks()[deck]?.tempo?.bpm;
    return bpm && trackBpm ? bpm / trackBpm : null;
  }

  /** Whether a touch of the controller's jog wheel of `deck` holds the record. */
  setJogScratch(deck: number, on: boolean) {
    return this.turn(`deck${deck + 1}.jog.scratch`, on);
  }

  /** `info` is on `deck` now: its waveform follows. */
  private show(deck: number, info: TrackInfo) {
    this.patch(this.tracks, deck, info);
    this.patch(this.waveforms, deck, null);
    this.watchWaveform(deck, info.loadId);
  }

  /** PLAY; pressed while CUE previews, the deck plays on once CUE is let go, as on a CDJ. */
  togglePlay(deck: number) {
    return this.press(deck, 'play');
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
    return this.call('set_pitch', { deck, ratio });
  }

  /** A deck put on a record takes the turntable from the deck layered with it, which goes
   * on by itself. */
  setVinyl(
    deck: number,
    changes: { enabled?: boolean; mode?: VinylMode; rpm?: Rpm; leadInS?: number | null },
  ) {
    const d = this.decks()[deck];
    return this.call('set_vinyl', {
      deck,
      enabled: changes.enabled ?? d.vinylEnabled,
      mode: changes.mode ?? d.vinylMode,
      rpm: changes.rpm ?? d.rpm,
      leadInS: changes.leadInS !== undefined ? changes.leadInS : d.leadInS,
    });
  }

  setVinylSwap(deck: number, swap: boolean) {
    return this.call('set_vinyl_swap', { deck, swap });
  }

  /** The record on the deck's turntable (a timecode format's id): on the deck layered with
   * it too, which reads the same turntable. */
  setTimecode(deck: number, format: string) {
    return this.call('set_timecode', { deck, format });
  }

  /**
   * SYNC button: unlocks a locked deck; otherwise locks it, or on a relative record in
   * one-shot mode matches it once (pressing again re-matches after a drift).
   */
  toggleSync(deck: number) {
    return this.perform(`deck${deck + 1}.sync`, 1);
  }

  /** -1, 0 or 1: slower, released, faster, as the bend keys. */
  pitchBend(deck: number, direction: -1 | 0 | 1) {
    return this.perform(
      `deck${deck + 1}.bend.${direction < 0 ? 'down' : 'up'}`,
      Math.abs(direction),
    );
  }

  /** Analysis and stems that arrive after the track was put on its deck. */
  private completeTrack({ deck, loadId, analysis, stems }: DeckTrackUpdate) {
    const track = this.tracks()[deck];
    if (track?.loadId !== loadId) return;
    if (analysis || stems) this.patch(this.tracks, deck, { ...track, ...analysis, ...stems });
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

  private refuse(text: string) {
    clearTimeout(this.refusalTimer);
    this.refusal.set(text);
    this.refusalTimer = setTimeout(() => this.refusal.set(null), REFUSAL_MS);
  }

  /** Has the backend perform action `target` given `value`: a key's 1 pressed and 0 let go, a
   * fader's position, or an encoder's steps (`steps`), as a controller's control would;
   * `confirmed`, the DJ said yes to what it replaces. Whether it was done. */
  perform(target: string, value: number, steps = false, confirmed = false) {
    return this.call('perform_action', { target, value, steps, step: null, confirmed });
  }

  /** The key of `action` of `deck` (`play`, `cue`…) pressed, or let go (`down` false). */
  press(deck: number, action: string, down = true) {
    return this.perform(`deck${deck + 1}.${action}`, down ? 1 : 0);
  }

  /** The switch `target` (`deck1.keylock`, `mixer.talkover`…) turned on or off. */
  turn(target: string, on: boolean) {
    return this.perform(`${target}.${on ? 'on' : 'off'}`, 1);
  }

  /** Runs a backend command; a failure is shown rather than thrown, until one goes through
   * (see `failed`). Whether it was done. */
  async call(command: string, args: Record<string, unknown>): Promise<boolean> {
    this.refusal.set(null);
    try {
      await invoke(command, args);
      this.error.set(null);
      return true;
    } catch (e) {
      return this.failed(e);
    }
  }

  /** What the backend did not do, for the app or a controller: the track of a deck that plays
   * stays there (said for a few seconds, not as an error), or leaves it once the DJ says so,
   * the action performed again then; any other failure is shown. Whether it was done in the
   * end. */
  async failed(error: unknown): Promise<boolean> {
    if (!isBackendError(error) || !['deckPlays', 'confirmReplace'].includes(error.code)) {
      this.error.set(errorText(error));
      return false;
    }
    const letter = { deck: deckLetter(Number(error.params['deck'])) };
    const eject = Boolean(error.params['eject']);
    if (error.code === 'deckPlays') {
      this.refuse(
        eject
          ? t('Deck {deck} is playing: its track stays on it', letter)
          : t('Deck {deck} is playing: nothing is loaded on it', letter),
      );
      return false;
    }
    const confirmed = await this.confirm.ask({
      title: t('Deck {deck} is playing', letter),
      detail: eject
        ? t('The track playing stops and leaves the deck.')
        : t('The track playing stops, and the new one takes its place.'),
      confirm: eject ? t('Eject') : t('Load'),
      cancel: t('Cancel'),
    });
    return confirmed && this.perform(String(error.params['target']), 1, false, true);
  }
}
