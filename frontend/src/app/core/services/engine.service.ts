import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';
import { StemPart, partId } from '../stem-parts';
import { Channel, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { transposeCamelot } from '../../shared/camelot';
import {
  ControlState,
  MAX_DECKS,
  DeckTrackUpdate,
  DEFAULT_FX,
  FLAT_STRIP,
  MusicalKey,
  DeckSettings,
  EngineStatus,
  SAMPLE_SLOTS,
  TrackInfo,
  VinylChange,
  Waveform,
  addWaveformPiece,
  decodeWaveform,
} from '../models/engine';
import { PreferencesService } from './preferences.service';
import { Playhead, followPlayhead } from '../playhead';
import { ConfirmService } from '../../shared/confirm/confirm';
import { same, sameItems } from '../same';
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
  readonly loading = computed(() => this.controls()?.loading ?? NONE_LOADING, { equal: sameItems });
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
  /** Why what the DJ just asked was not done, nothing being wrong: said for a few seconds,
   * or until the next thing asked. */
  readonly refusal = signal<string | null>(null);
  private refusalTimer?: ReturnType<typeof setTimeout>;
  /** This page's load, and how many actions it sent: the backend performs them in that order,
   * whatever order their calls reach it in. */
  private readonly run = Math.floor(Math.random() * Number.MAX_SAFE_INTEGER);
  private sent = 0;

  readonly audioRunning = computed(() => this.status()?.audioRunning ?? false);

  constructor() {
    // what failed while the audio was away is behind once it runs again
    effect(() => {
      if (this.audioRunning()) untracked(() => this.error.set(null));
    });
  }

  /** Where each deck's playhead was when, and its speed: what it is drawn from. */
  private playheads: (Playhead | null)[] = [];

  /** Subscribes to the engine state: done once its first state came, which tells all the app
   * mirrors (the decks' settings, the mixer's…). */
  async connect(): Promise<void> {
    const channel = new Channel<EngineStatus>();
    let mirrored: () => void;
    const told = new Promise<void>((resolve) => (mirrored = resolve));
    channel.onmessage = (s) => {
      this.playheads = s.decks.map((d, i) => followPlayhead(this.playheads[i] ?? null, d));
      if (s.controls) {
        this.applyControls(s.controls);
        mirrored();
      }
      this.status.set(s);
    };
    await invoke('subscribe_status', { channel });
    await listen<DeckTrackUpdate>('deck-track-update', (e) => this.completeTrack(e.payload));
    await told;
  }

  /** The sections the backend changed of what the app mirrors (`changed`, the first time all
   * of them): the decks' settings, their tracks (one gone leaves its deck, one not shown yet is
   * asked for) with their key shifts, stem mixes and cues, the DJ's quantize and tempo range.
   * The sections not sent, and what is the same, stay as they are, for the views that follow
   * them. */
  private applyControls(changed: Partial<ControlState>) {
    const controls = { ...this.controls(), ...changed } as ControlState;
    if (changed.mixer) {
      const decks = changed.mixer.decks.map((d, deck) => {
        const next = { ...DEFAULT_DECK, ...d };
        return same(next, this.decks()[deck]) ? this.decks()[deck] : next;
      });
      if (decks.some((d, deck) => d !== this.decks()[deck])) this.decks.set(decks);
      const { quantize, pitchRange } = controls;
      const dj = this.preferences.dj();
      if (dj.quantize !== quantize || dj.pitchRange !== pitchRange)
        this.preferences.dj.set({ ...dj, quantize, pitchRange });
    }
    if (changed.tracks) {
      const told = changed.tracks;
      const tracks = this.tracks().map((track, deck) => {
        if (!told[deck]) return null;
        if (track?.loadId !== told[deck].loadId) return track;
        const { keyShift, stemMix, cues } = told[deck];
        const next = { ...track, keyShift, stemMix, cues };
        return same(next, track) ? track : next;
      });
      if (tracks.some((t, deck) => t !== this.tracks()[deck])) {
        this.tracks.set(tracks);
        this.waveforms.update((waveforms) => waveforms.map((w, deck) => (tracks[deck] ? w : null)));
      }
    }
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

  /** Matches the leader once and leaves the deck free (unlocking it first); a deck playing by
   * itself takes the tempo on its fader. */
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

  /** The deck `deck` is mixed with: another one playing, else the one beside it (A and B, C
   * and D). */
  mixedWith(deck: number): number {
    const playing = (this.status()?.decks ?? []).findIndex((d, i) => i !== deck && d.playing);
    return playing >= 0 ? playing : deck ^ 1;
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

  /** The track on `deck` put at `seconds`, as its place along it (`position`). */
  seek(deck: number, seconds: number) {
    const duration = this.status()?.decks[deck]?.durationS;
    if (!duration) return Promise.resolve(false);
    const at = Math.min(Math.max(seconds / duration, 0), 1);
    return this.perform(`deck${deck + 1}.position`, at);
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

  /** The tempo fader of `deck` moved to `offset` from the track's tempo (0.02: +2 %), within
   * its range. */
  moveTempo(deck: number, offset: number) {
    return this.perform(`deck${deck + 1}.pitch`, 0.5 + offset / (2 * this.tempoRange(deck)));
  }

  /** A deck put on a record (on) takes the turntable from the deck layered with it, which goes
   * on by itself. */
  setVinylEnabled(deck: number, on: boolean) {
    return this.turn(`deck${deck + 1}.vinyl`, on);
  }

  /** A setting of the vinyl control of `deck` changed alone. */
  setVinyl(deck: number, change: VinylChange) {
    return this.call('set_vinyl', { deck, change });
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
    const order = { run: this.run, number: ++this.sent };
    return this.call('perform_action', { target, value, steps, confirmed, order });
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
