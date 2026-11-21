import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';
import { Channel, invoke } from '@tauri-apps/api/core';
import { ControlState, MAX_DECKS, StatusChanges, Waveform, decodeWaveform } from '../models/engine';
import { Playhead, followPlayhead } from '../playhead';
import { ConfirmService } from '../../shared/confirm/confirm';
import { same, sameItems } from '../same';
import { withChanges } from '../told';
import { deckLetter } from '../../shared/time';
import { t } from '../i18n/i18n';
import { errorText, isBackendError } from '../errors';
import type {
  ControlChanges,
  DeckSettings,
  DeckStatus,
  EngineStatus,
  StemPart,
  TrackInfo,
  VinylChange,
} from '../models/backend';

/** How long a refusal stays said. */
const REFUSAL_MS = 5000;
const NONE_LOADING: boolean[] = Array(MAX_DECKS).fill(false);

/**
 * Bridge to the Rust engine: commands go through `invoke`, the engine state arrives
 * ~60 times per second through a Tauri channel.
 */
@Injectable({ providedIn: 'root' })
export class EngineService {
  private readonly confirm = inject(ConfirmService);

  /** Latest engine state (drives the templates). */
  readonly status = signal<EngineStatus | null>(null);
  /** What the app mirrors as the backend last told it changed (`StatusChanges.controls`). */
  readonly controls = signal<ControlState | null>(null);
  /** What the app mirrors, once the backend told it: the shell shows from then on (`ready`),
   * and reads it only there. */
  readonly told = computed(() => this.controls()!);
  readonly ready = computed(() => this.controls() !== null);
  readonly tracks = signal<(TrackInfo | null)[]>(Array(MAX_DECKS).fill(null));
  /** Whether each deck loads a track. */
  readonly loading = computed(() => this.controls()?.loading ?? NONE_LOADING, { equal: sameItems });
  /** Waveform of each deck's track, once fetched. */
  readonly waveforms = signal<(Waveform | null)[]>(Array(MAX_DECKS).fill(null));
  /** Each deck's settings, as the backend told them first with its first status. */
  readonly decks = signal<DeckSettings[]>([]);
  /** The decks shown and played, two or four, as the backend tells them; none before its
   * first status. */
  readonly deckIndexes = computed(() => (this.status()?.decks ?? []).map((_, deck) => deck), {
    equal: sameItems,
  });
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

  /** The status of `deck` at the last frame told: none before the first, nor for a deck not
   * in use. */
  deckStatus(deck: number): DeckStatus | null {
    return this.status()?.decks[deck] ?? null;
  }

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
    const channel = new Channel<StatusChanges>();
    let mirrored: () => void;
    const told = new Promise<void>((resolve) => (mirrored = resolve));
    channel.onmessage = ({ controls, ...changes }) => {
      const s = withChanges(this.status(), changes);
      this.playheads = s.decks.map((d, i) => followPlayhead(this.playheads[i] ?? null, d));
      if (controls) {
        this.applyControls(controls);
        mirrored();
      }
      this.status.set(s);
    };
    await invoke('subscribe_status', { channel });
    await told;
  }

  /** The sections the backend changed of what the app mirrors (`changed`, the first time all
   * of them), each whole: the decks' settings, their tracks (a new load's waveform followed),
   * the DJ's quantize and tempo range. The sections not sent, and the decks and tracks that
   * are the same, stay as they are, for the views that follow them. */
  private applyControls(changed: ControlChanges) {
    const decks = changed.decks?.map((d, deck) =>
      same(d, this.decks()[deck]) ? this.decks()[deck] : d,
    );
    const controls = { ...this.controls(), ...changed, ...(decks && { decks }) } as ControlState;
    if (decks?.some((d, deck) => d !== this.decks()[deck])) this.decks.set(decks);
    if (changed.tracks) {
      const before = this.tracks();
      const tracks = changed.tracks.map((track, deck) =>
        same(track, before[deck]) ? before[deck] : track,
      );
      if (tracks.some((t, deck) => t !== before[deck])) {
        this.tracks.set(tracks);
        tracks.forEach((track, deck) => {
          if (track?.loadId === before[deck]?.loadId) return;
          this.patch(this.waveforms, deck, null);
          if (track) this.watchWaveform(deck, track.loadId);
        });
      }
    }
    this.controls.set(controls);
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

  /** Mutes or plays a part of the track on `deck` (until the next load), on the next bar
   * with quantize. */
  toggleStem(deck: number, part: StemPart) {
    return this.press(deck, part);
  }

  /** Sets how loud a part of the track on `deck` plays (0 to 1, until the next load). */
  setStemLevel(deck: number, part: StemPart, level: number) {
    return this.perform(`deck${deck + 1}.${part}.level`, level);
  }

  setKeyLock(deck: number, enabled: boolean) {
    return this.turn(`deck${deck + 1}.keylock`, enabled);
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

  /** PLAY; pressed while CUE previews, the deck plays on once CUE is let go, as on a CDJ. */
  togglePlay(deck: number) {
    return this.press(deck, 'play');
  }

  /** The track on `deck` put at `seconds`. */
  seek(deck: number, seconds: number) {
    return this.call('seek_deck', { deck, seconds });
  }

  /** The track dragged by hand to `seconds` at `atMs` (the pointer event's time), as the
   * scrolling waveform is moved like a record, or let go (`null`). */
  hand(deck: number, seconds: number | null, atMs = performance.now()) {
    return this.call('hand', { deck, seconds, atMs });
  }

  /** The edge of a jog wheel turned by `seconds` of track (positive forwards) at `atMs`: a
   * playing track pushed or held back for a moment, a stopped one moved. */
  jog(deck: number, seconds: number, atMs = performance.now()) {
    return this.call('jog', { deck, seconds, atMs });
  }

  /** The tempo fader of `deck` moved to `position` (0 to 1, its middle the track's tempo). */
  moveTempo(deck: number, position: number) {
    return this.perform(`deck${deck + 1}.pitch`, position);
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

  /** -1, 0 or 1: slower, released, faster, as the bend keys. */
  pitchBend(deck: number, direction: -1 | 0 | 1) {
    return this.perform(
      `deck${deck + 1}.bend.${direction < 0 ? 'down' : 'up'}`,
      Math.abs(direction),
    );
  }

  /** The waveform of the load `loadId` on `deck`: what is known of it at once, then, while the
   * track is read, what changed of it as it grows. */
  private watchWaveform(deck: number, loadId: number) {
    const channel = new Channel<ArrayBuffer>();
    let shown: Waveform | null = null;
    channel.onmessage = (buffer) => {
      shown = decodeWaveform(buffer, shown) ?? shown;
      if (shown && this.tracks()[deck]?.loadId === loadId) this.patch(this.waveforms, deck, shown);
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
   * fader's position, or an encoder's steps, as a controller's control would;
   * `confirmed`, the DJ said yes to what it replaces, the track of the library it takes the one
   * the DJ was asked about (`track`), else the one selected. Whether it was done. */
  perform(target: string, value: number, confirmed = false, track: string | null = null) {
    return this.call('perform_action', { target, value, confirmed, track, order: this.order() });
  }

  /** Loads the track at `path` on `deck`, or on the first deck without a track (`null`); asked
   * again once the DJ says yes to what it replaces. Whether it was done. */
  async loadTrack(path: string, deck: number | null, confirmed = false): Promise<boolean> {
    this.refusal.set(null);
    try {
      await this.ordered('load_track', { path, deck, confirmed });
      this.error.set(null);
      return true;
    } catch (e) {
      return this.failed(e, () => this.loadTrack(path, deck, true));
    }
  }

  /** Runs the backend command `command` in turn with the actions sent before it, as an action
   * is: what a gesture on the library does. */
  ordered<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
    return invoke<T>(command, { ...args, order: this.order() });
  }

  /** The place of the next action among those this page sent. */
  private order() {
    return { run: this.run, number: ++this.sent };
  }

  /** The key of `action` of `deck` (`play`, `cue`…) pressed, or let go (`down` false). */
  press(deck: number, action: string, down = true) {
    return this.perform(`deck${deck + 1}.${action}`, down ? 1 : 0);
  }

  /** The key `action` of `deck` pressed and lost (gone from under the pointer): let go without
   * what its release does (SYNC tapped). */
  letGo(deck: number, action: string) {
    return this.call('let_go_key', { target: `deck${deck + 1}.${action}`, order: this.order() });
  }

  /** The key `action` of `deck` let go, or its press lost (`letGo`). */
  release(deck: number, action: string, letGo: boolean) {
    return letGo ? this.press(deck, action, false) : this.letGo(deck, action);
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
   * the action done `again` then (by default, performed again on the track it took); any other
   * failure is shown.
   * Whether it was done in the end. */
  async failed(error: unknown, again?: () => Promise<boolean>): Promise<boolean> {
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
    if (!confirmed) return false;
    const track = error.params['track'];
    return again
      ? again()
      : this.perform(
          String(error.params['target']),
          1,
          true,
          typeof track === 'string' ? track : null,
        );
  }
}
