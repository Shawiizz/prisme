import { Injectable, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { EngineService } from './engine.service';
import { attempt, errorText } from '../errors';
import type { Cue, PadPageNames } from '../models/backend';

/** Performance actions on a deck: hot cues, the pads of a page (loops, rolls, beat jumps, the
 * slicer, fader cuts), slip, censor, reverse, the motor (brake, spinback). */
@Injectable({ providedIn: 'root' })
export class PerformanceService {
  private readonly engine = inject(EngineService);
  /** The pages of pads named by their action, as the backend sets them out (`loadPages`). */
  readonly pages = signal<PadPageNames[]>([]);
  private loading: Promise<void> | null = null;

  /** Asks the backend for the pages once: again next time, if it failed. */
  loadPages() {
    this.loading ??= invoke<PadPageNames[]>('pad_pages').then(
      (pages) => this.pages.set(pages),
      (e) => {
        this.loading = null;
        this.engine.error.set(errorText(e));
      },
    );
    return this.loading;
  }

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

  /** The action of a pad of `deck` pressed (`held`) or let go. */
  pad(deck: number, action: string, held: boolean) {
    return this.engine.press(deck, action, held);
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
    return this.engine.perform(`deck${deck + 1}.size`, steps);
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
  anchorSlicer(deck: number) {
    return attempt(this.engine.error, () => invoke('anchor_slicer', { deck }));
  }
}
