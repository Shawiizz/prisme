import { Injectable, computed, inject } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { Cue, DeckPerform, MAX_DECKS, SLICES } from '../models/engine';
import { EngineService } from './engine.service';
import { errorText } from '../errors';
import { sameItems } from '../same';

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
      Array(MAX_DECKS).fill({ slicer: null, cut: null, roll: null, fxFocus: null }),
  );
  /** Where each deck's slicer starts (s): on the bar it played in when it was put there. */
  readonly slicers = computed(() => this.held().map((d) => d.slicer), { equal: sameItems });
  /** The fader cut each deck plays (an index of `CUT_BEATS`). */
  readonly cuts = computed(() => this.held().map((d) => d.cut), { equal: sameItems });
  /** The roll each deck plays, in beats. */
  readonly rolls = computed(() => this.held().map((d) => d.roll), { equal: sameItems });

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

  /** A loop of `beats` on `deck`, or out of it if it plays that loop. */
  beatLoop(deck: number, beats: number) {
    return this.engine.press(deck, `loop.${beats}`);
  }

  /** Roll `index` (of `ROLL_BEATS`) while the pad is `held`. */
  roll(deck: number, index: number, held: boolean) {
    return this.engine.press(deck, `roll${index + 1}`, held);
  }

  /** Marks where the next loop starts. */
  loopIn(deck: number) {
    return this.engine.press(deck, 'loop.in');
  }

  /** Ends the loop at the playhead and plays it (from LOOP IN, else the stored loop). */
  loopOut(deck: number) {
    return this.engine.press(deck, 'loop.out');
  }

  /** The stored loop played again, or left while it plays. */
  reloop(deck: number) {
    return this.engine.press(deck, 'reloop');
  }

  /** The loop halved (`factor` 0.5) or doubled (2). */
  scaleLoop(deck: number, factor: 0.5 | 2) {
    return this.engine.press(deck, factor < 1 ? 'loop.halve' : 'loop.double');
  }

  /** The size of `deck` halved (-1) or doubled (1) so many times, from 1/16 beat to 64. */
  resize(deck: number, steps: number) {
    return this.engine.perform(`deck${deck + 1}.size`, steps, true);
  }

  beatJump(deck: number, beats: number) {
    return this.engine.press(deck, `jump.${beats}`);
  }

  setSlip(deck: number, enabled: boolean) {
    return this.engine.turn(`deck${deck + 1}.slip`, enabled);
  }

  /** The track muted, playing on underneath, while `active`. */
  censor(deck: number, active: boolean) {
    return this.engine.press(deck, 'censor', active);
  }

  /** The track played backwards at the deck's rate, or forwards again; with slip, it comes
   * back where it would be. */
  setReverse(deck: number, enabled: boolean) {
    return this.engine.turn(`deck${deck + 1}.reverse`, enabled);
  }

  /** A playing deck slows to a stop as a turntable switched off; a stopped one starts softly. */
  brake(deck: number) {
    return this.engine.press(deck, 'brake');
  }

  /** The record spun back while held; let go, it plays on from there. */
  spinBack(deck: number, held: boolean) {
    return this.engine.press(deck, 'spinback', held);
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
