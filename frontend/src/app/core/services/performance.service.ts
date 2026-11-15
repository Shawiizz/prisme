import { Injectable, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { Cue, MAX_DECKS } from '../models/engine';
import { EngineService } from './engine.service';
import { errorText } from '../errors';

/** The shortest and longest size of a deck, in beats (see `PerformanceService.sizes`). */
export const SIZE_BEATS = { min: 1 / 16, max: 64 };
const DEFAULT_SIZE = 4;

/** Performance actions on a deck: hot cues, loops, rolls, beat jumps, slip, censor, the
 * motor (brake, spinback). */
@Injectable({ providedIn: 'root' })
export class PerformanceService {
  private readonly engine = inject(EngineService);
  /** Each deck's size in beats: what its jumps go by, what its sized loop lasts. */
  readonly sizes = signal<number[]>(Array(MAX_DECKS).fill(DEFAULT_SIZE));

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
    const size = Math.min(Math.max(this.size(deck) * factor, SIZE_BEATS.min), SIZE_BEATS.max);
    this.sizes.update((sizes) => sizes.map((s, d) => (d === deck ? size : s)));
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

  /** A playing deck slows to a stop as a turntable switched off; a stopped one starts softly. */
  brake(deck: number) {
    return this.engine.call('brake', { deck });
  }

  /** The record spun back while held; let go, it plays on from there. */
  spinBack(deck: number, held: boolean) {
    return this.engine.call('spin_back', { deck, held });
  }

  private async updateCues(deck: number, command: string, slot: number) {
    try {
      this.engine.setCues(deck, await invoke<Cue[]>(command, { deck, slot }));
    } catch (e) {
      this.engine.error.set(errorText(e));
    }
  }
}
