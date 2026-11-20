import { Injectable, computed, inject } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { Cue, DeckPerform, MAX_DECKS, SLICES } from '../models/engine';
import { EngineService } from './engine.service';
import { errorText } from '../errors';

/** Performance actions on a deck: hot cues, loops, rolls, beat jumps, the slicer, fader
 * cuts, slip, censor, reverse, the motor (brake, spinback). */
@Injectable({ providedIn: 'root' })
export class PerformanceService {
  private readonly engine = inject(EngineService);
  /** Each deck's size in beats: what its jumps go by, what its sized loop lasts. */
  readonly sizes = computed(() => this.engine.decks().map((d) => d.sizeBeats));
  /** What each deck's pads hold, as the backend keeps it. */
  private readonly held = computed<DeckPerform[]>(
    () =>
      this.engine.controls()?.perform ??
      Array(MAX_DECKS).fill({ slicer: null, cut: null, roll: null }),
  );
  /** Where each deck's slicer starts (s): on the bar it played in when it was put there. */
  readonly slicers = computed(() => this.held().map((d) => d.slicer));
  /** The fader cut each deck plays (an index of `CUT_BEATS`). */
  readonly cuts = computed(() => this.held().map((d) => d.cut));
  /** The roll each deck plays, in beats. */
  readonly rolls = computed(() => this.held().map((d) => d.roll));

  hotCue(deck: number, slot: number): Cue | null {
    return this.engine.tracks()[deck]?.cues.find((c) => c.hot === slot) ?? null;
  }

  /** Jumps to the hot cue, or sets it there if the slot is empty. */
  pressHotCue(deck: number, slot: number) {
    return this.engine.press(deck, `hotcue${slot + 1}`);
  }

  deleteHotCue(deck: number, slot: number) {
    return this.engine.press(deck, `hotcue${slot + 1}.delete`);
  }

  beatLoop(deck: number, beats: number) {
    return this.engine.call('beat_loop', { deck, beats });
  }

  /** Roll `index` (of `ROLL_BEATS`) while the pad is `held`. */
  roll(deck: number, index: number, held: boolean) {
    return this.engine.press(deck, `roll${index + 1}`, held);
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

  /** The size of `deck` halved (-1) or doubled (1) so many times, from 1/16 beat to 64. */
  resize(deck: number, steps: number) {
    return this.engine.perform(`deck${deck + 1}.size`, steps, true);
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
  async anchorSlicer(deck: number) {
    try {
      await invoke('anchor_slicer', { deck });
    } catch (e) {
      this.engine.error.set(errorText(e));
    }
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

  /** Plays slice `index` of the slicer round and round while `held`, the track going on
   * underneath; of the slices held, the last pressed. */
  slice(deck: number, index: number, held: boolean) {
    return this.engine.press(deck, `slice${index + 1}`, held);
  }

  /** Cuts the deck on the beats of fader cut `index` (of `CUT_BEATS`) while `held`; of the
   * cuts held, the last pressed. */
  cut(deck: number, index: number, held: boolean) {
    return this.engine.press(deck, `cut${index + 1}`, held);
  }

  /** The first beat of the track on `deck` and how long a beat lasts (s). */
  private beatGrid(deck: number): { firstBeatS: number; beatS: number } | null {
    const tempo = this.engine.tracks()[deck]?.tempo;
    return tempo ? { firstBeatS: tempo.firstBeatS, beatS: 60 / tempo.bpm } : null;
  }
}
