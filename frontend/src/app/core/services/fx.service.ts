import { Injectable, inject } from '@angular/core';
import { EffectKind, FX_BEATS, FxSettings } from '../models/engine';
import { EngineService } from './engine.service';

/** Each deck's beat effect: which one, on or off, its level, its knob and its period. */
@Injectable({ providedIn: 'root' })
export class FxService {
  private readonly engine = inject(EngineService);

  settings(deck: number): FxSettings {
    return this.engine.decks()[deck].fx;
  }

  /** Switches to `kind` and on; pressed again, switches it off. */
  press(deck: number, kind: EffectKind) {
    const fx = this.settings(deck);
    if (fx.kind === kind && fx.enabled) return this.setEnabled(deck, false);
    if (fx.kind !== kind) {
      this.patch(deck, { kind });
      this.engine.call('set_fx', { deck, kind });
    }
    return this.setEnabled(deck, true);
  }

  setEnabled(deck: number, enabled: boolean) {
    this.patch(deck, { enabled });
    return this.engine.call('set_fx_enabled', { deck, enabled });
  }

  setMix(deck: number, mix: number) {
    this.patch(deck, { mix });
    return this.engine.call('set_fx_mix', { deck, mix });
  }

  setAmount(deck: number, amount: number) {
    this.patch(deck, { amount });
    return this.engine.call('set_fx_amount', { deck, amount });
  }

  /** Halves (0.5) or doubles (2) the effect's period. */
  scaleBeats(deck: number, factor: number) {
    const beats = Math.min(
      Math.max(this.settings(deck).beats * factor, FX_BEATS.min),
      FX_BEATS.max,
    );
    this.patch(deck, { beats });
    return this.engine.call('set_fx_beats', { deck, beats });
  }

  private patch(deck: number, changes: Partial<FxSettings>) {
    this.engine.patchDeck(deck, { fx: { ...this.settings(deck), ...changes } });
  }
}
