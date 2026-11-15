import { Injectable, effect, inject, untracked } from '@angular/core';
import { EFFECT_KINDS, EffectKind, FX_BEATS, FxSettings } from '../models/engine';
import { StemPart, hasPart } from '../stem-parts';
import { EngineService } from './engine.service';

/**
 * Each deck's beat effect: which one, on or off, its level, its knob, its period, and the
 * part of the track it is on. A track loaded without that part puts it back on the whole
 * deck, where it is heard.
 */
@Injectable({ providedIn: 'root' })
export class FxService {
  private readonly engine = inject(EngineService);

  constructor() {
    effect(() => {
      const tracks = this.engine.tracks();
      untracked(() => {
        tracks.forEach((track, deck) => {
          const part = this.settings(deck)?.part;
          if (track && part && !hasPart(track.separation, part)) void this.setPart(deck, null);
        });
      });
    });
  }

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

  /** The effect `step` places after this one (before it, negative), on or off as it was. */
  cycle(deck: number, step: number) {
    const at = EFFECT_KINDS.indexOf(this.settings(deck).kind);
    const kind = EFFECT_KINDS[(at + step + EFFECT_KINDS.length * 2) % EFFECT_KINDS.length];
    this.patch(deck, { kind });
    return this.engine.call('set_fx', { deck, kind });
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

  /** The parts the effect can be on alone, as the deck's track is separated, after the
   * whole deck (`null`). */
  parts(deck: number): (StemPart | null)[] {
    switch (this.engine.tracks()[deck]?.separation ?? 'none') {
      case 'none':
        return [null];
      case 'vocals':
        return [null, 'vocals', 'instrumental'];
      case 'full':
        return [null, 'vocals', 'drums', 'bass', 'other'];
    }
  }

  setPart(deck: number, part: StemPart | null) {
    this.patch(deck, { part });
    return this.engine.call('set_fx_part', { deck, part });
  }

  /** The effect on the next part it can be on, after the last on the whole deck again. */
  nextPart(deck: number) {
    const parts = this.parts(deck);
    const at = parts.indexOf(this.settings(deck).part);
    return this.setPart(deck, parts[(at + 1) % parts.length]);
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
