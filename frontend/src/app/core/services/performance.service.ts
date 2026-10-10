import { Injectable, inject } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { Cue } from '../models/engine';
import { EngineService } from './engine.service';

/** Performance actions on a deck: hot cues, loops, rolls, beat jumps, slip, censor. */
@Injectable({ providedIn: 'root' })
export class PerformanceService {
  private readonly engine = inject(EngineService);

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

  exitLoop(deck: number) {
    return this.engine.call('exit_loop', { deck });
  }

  reloop(deck: number) {
    return this.engine.call('reloop', { deck });
  }

  scaleLoop(deck: number, factor: number) {
    return this.engine.call('scale_loop', { deck, factor });
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

  private async updateCues(deck: number, command: string, slot: number) {
    try {
      this.engine.setCues(deck, await invoke<Cue[]>(command, { deck, slot }));
    } catch (e) {
      this.engine.error.set(String(e));
    }
  }
}
