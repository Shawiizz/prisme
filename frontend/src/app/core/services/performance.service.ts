import { Injectable, computed, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { Cue, MAX_DECKS, SLICES } from '../models/engine';
import { EngineService } from './engine.service';
import { errorText } from '../errors';
import { CUT_BEATS } from '../../shared/beats';

/** The shortest and longest size of a deck, in beats (see `PerformanceService.sizes`). */
export const SIZE_BEATS = { min: 1 / 16, max: 64 };
const BEATS_PER_BAR = 4;

/** Performance actions on a deck: hot cues, loops, rolls, beat jumps, the slicer, fader
 * cuts, slip, censor, reverse, the motor (brake, spinback). */
@Injectable({ providedIn: 'root' })
export class PerformanceService {
  private readonly engine = inject(EngineService);
  /** Each deck's size in beats: what its jumps go by, what its sized loop lasts. */
  readonly sizes = computed(() => this.engine.decks().map((d) => d.sizeBeats));
  /** Where each deck's slicer starts (s): on the bar it played in when it was put there. */
  readonly slicers = signal<(number | null)[]>(Array(MAX_DECKS).fill(null));
  /** The slices held on each deck, the last pressed last: it plays. */
  private readonly slicesHeld: number[][] = Array.from({ length: MAX_DECKS }, () => []);
  /** The fader cut each deck plays (an index of `CUT_BEATS`), and those held. */
  readonly cuts = signal<(number | null)[]>(Array(MAX_DECKS).fill(null));
  private readonly cutsHeld: number[][] = Array.from({ length: MAX_DECKS }, () => []);

  hotCue(deck: number, slot: number): Cue | null {
    return this.engine.tracks()[deck]?.cues.find((c) => c.hot === slot) ?? null;
  }

  /** Jumps to the hot cue, or sets it there if the slot is empty. */
  pressHotCue(deck: number, slot: number) {
    return this.hotCue(deck, slot)
      ? this.engine.call('trigger_hot_cue', { deck, slot })
      : this.updateCues(deck, 'set_hot_cue', slot);
  }

  deleteHotCue(deck: number, slot: number) {
    return this.updateCues(deck, 'delete_hot_cue', slot);
  }

  beatLoop(deck: number, beats: number) {
    return this.engine.call('beat_loop', { deck, beats });
  }

  /** `beats` while the pad is held, `null` when it is released. */
  loopRoll(deck: number, beats: number | null) {
    return this.engine.call('loop_roll', { deck, beats });
  }

  /** Marks where the next loop starts. */
  loopIn(deck: number) {
    return this.engine.call('loop_in', { deck });
  }

  /** Ends the loop at the playhead and plays it (from LOOP IN, else the stored loop). */
  loopOut(deck: number) {
    return this.engine.call('loop_out', { deck });
  }

  exitLoop(deck: number) {
    return this.engine.call('exit_loop', { deck });
  }

  reloop(deck: number) {
    return this.engine.call('reloop', { deck });
  }

  scaleLoop(deck: number, factor: number) {
    return this.engine.call('scale_loop', { deck, factor });
  }

  size(deck: number): number {
    return this.sizes()[deck];
  }

  /** The size of `deck` multiplied by `factor` (halved, doubled), within `SIZE_BEATS`. */
  scaleSize(deck: number, factor: number) {
    const beats = Math.min(Math.max(this.size(deck) * factor, SIZE_BEATS.min), SIZE_BEATS.max);
    this.engine.patchDeck(deck, { sizeBeats: beats });
    return this.engine.call('set_deck_size', { deck, beats });
  }

  /** A loop of the deck's size where it plays, or out of the loop it plays. */
  toggleSizedLoop(deck: number) {
    return this.engine.status()?.decks[deck]?.looping.active
      ? this.exitLoop(deck)
      : this.beatLoop(deck, this.size(deck));
  }

  beatJump(deck: number, beats: number) {
    return this.engine.call('beat_jump', { deck, beats });
  }

  setSlip(deck: number, enabled: boolean) {
    return this.engine.call('set_slip', { deck, enabled });
  }

  censor(deck: number, active: boolean) {
    return this.engine.call('censor', { deck, active });
  }

  /** The very end of the track: playing, the deck stops there (or starts again on repeat). */
  toEnd(deck: number) {
    const duration = this.engine.status()?.decks[deck]?.durationS ?? 0;
    if (duration > 0) this.engine.seek(deck, duration);
  }

  /** The track played backwards at the deck's rate, or forwards again; with slip, it comes
   * back where it would be. */
  setReverse(deck: number, enabled: boolean) {
    return this.engine.call('set_reverse', { deck, enabled });
  }

  /** A playing deck slows to a stop as a turntable switched off; a stopped one starts softly. */
  brake(deck: number) {
    return this.engine.call('brake', { deck });
  }

  /** The record spun back while held; let go, it plays on from there. */
  spinBack(deck: number, held: boolean) {
    return this.engine.call('spin_back', { deck, held });
  }

  /** Puts the slicer of `deck` on the bar the track is in (under a loop, where it would be). */
  anchorSlicer(deck: number) {
    const grid = this.beatGrid(deck);
    const d = this.engine.status()?.decks[deck];
    if (!grid || !d?.loaded) return;
    const bar = BEATS_PER_BAR * grid.beatS;
    const position = d.slipPositionS ?? d.positionS;
    const start = grid.firstBeatS + Math.floor((position - grid.firstBeatS) / bar) * bar;
    this.slicers.update((starts) => starts.map((s, i) => (i === deck ? start : s)));
  }

  /** The slice of its slicer `deck` plays in, if it plays in it. */
  slicePlaying(deck: number): number | null {
    const start = this.slicers()[deck];
    const grid = this.beatGrid(deck);
    const d = this.engine.status()?.decks[deck];
    if (start === null || !grid || !d?.loaded) return null;
    const slice = Math.floor((d.positionS - start) / grid.beatS);
    return slice >= 0 && slice < SLICES ? slice : null;
  }

  /** Plays slice `index` of the slicer round and round while held, the track going on
   * underneath; of the slices held, the last pressed. A slicer the track has left is put on
   * the bar it is in at the first press. */
  slice(deck: number, index: number, held: boolean) {
    const stack = this.slicesHeld[deck];
    if (held && stack.length === 0 && !this.inSlicer(deck)) this.anchorSlicer(deck);
    const [playing, slice] = press(stack, index, held);
    if (slice === playing) return;
    const start = this.slicers()[deck];
    const grid = this.beatGrid(deck);
    if (slice === undefined || start === null || !grid) {
      return this.engine.call('slice', { deck, startS: null, lengthS: 0 });
    }
    const startS = start + slice * grid.beatS;
    return this.engine.call('slice', { deck, startS, lengthS: grid.beatS });
  }

  /** Cuts the deck on the beats of fader cut `index` (of `CUT_BEATS`) while held; of the
   * cuts held, the last pressed. */
  cut(deck: number, index: number, held: boolean) {
    const [playing, cut] = press(this.cutsHeld[deck], index, held);
    if (cut === playing) return;
    this.cuts.update((cuts) => cuts.map((c, d) => (d === deck ? (cut ?? null) : c)));
    const beats = cut === undefined ? null : CUT_BEATS[cut];
    return this.engine.call('fader_cut', { deck, beats });
  }

  /** Whether the track (where it would be under a loop) is in the slicer of `deck`. */
  private inSlicer(deck: number): boolean {
    const start = this.slicers()[deck];
    const grid = this.beatGrid(deck);
    const d = this.engine.status()?.decks[deck];
    if (start === null || !grid || !d) return false;
    const position = d.slipPositionS ?? d.positionS;
    return position >= start && position < start + SLICES * grid.beatS;
  }

  /** The first beat of the track on `deck` and how long a beat lasts (s). */
  private beatGrid(deck: number): { firstBeatS: number; beatS: number } | null {
    const tempo = this.engine.tracks()[deck]?.tempo;
    return tempo ? { firstBeatS: tempo.firstBeatS, beatS: 60 / tempo.bpm } : null;
  }

  private async updateCues(deck: number, command: string, slot: number) {
    try {
      this.engine.setCues(deck, await invoke<Cue[]>(command, { deck, slot }));
    } catch (e) {
      this.engine.error.set(errorText(e));
    }
  }
}

/** A key of `stack` (those held, the last pressed last) pressed or released: the one that
 * acted before, and the one that acts now. */
function press(
  stack: number[],
  key: number,
  held: boolean,
): [number | undefined, number | undefined] {
  const before = stack.at(-1);
  const at = stack.indexOf(key);
  if (at >= 0) stack.splice(at, 1);
  if (held) stack.push(key);
  return [before, stack.at(-1)];
}
