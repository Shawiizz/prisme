import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';
import { EffectKind, FxChain, FxSettings, MAX_DECKS } from '../models/engine';
import { fxId } from '../control/controls';
import { EngineService } from './engine.service';
import { sameItems } from '../same';

/**
 * Each deck's chain of beat effects: in each slot, which effect, on or off, its level, its
 * two knobs and its period; the whole chain on or off; the part of the track the chain is
 * on; the slot the pads and knobs of the screen act on, and the slot a controller's knobs
 * are focused on, if any.
 */
@Injectable({ providedIn: 'root' })
export class FxService {
  private readonly engine = inject(EngineService);
  private readonly chosenSlots = signal<number[]>(Array(MAX_DECKS).fill(0));
  /** The slot each deck's effect knobs are focused on, as the backend keeps it. */
  private readonly focusedSlots = computed<(number | null)[]>(
    () => this.engine.controls()?.perform.map((d) => d.fxFocus) ?? Array(MAX_DECKS).fill(null),
    { equal: sameItems },
  );

  constructor() {
    // a slot focused is chosen on the screen too
    effect(() => {
      const focused = this.focusedSlots();
      untracked(() =>
        focused.forEach((slot, deck) => {
          if (slot !== null) this.choose(deck, slot);
        }),
      );
    });
  }

  chain(deck: number): FxChain {
    return this.engine.decks()[deck].fx;
  }

  settings(deck: number, slot: number): FxSettings {
    return this.chain(deck).slots[slot];
  }

  /** The slot the pads and knobs of the screen act on. */
  chosen(deck: number): number {
    return this.chosenSlots()[deck];
  }

  choose(deck: number, slot: number) {
    this.chosenSlots.update((slots) => slots.map((s, d) => (d === deck ? slot : s)));
  }

  /** The slot a controller's knobs act on as Mixxx's focused effect, if one is. */
  focused(deck: number): number | null {
    return this.focusedSlots()[deck];
  }

  /** Puts `kind` in the slot and switches it on; pressed again, switches it off. */
  press(deck: number, slot: number, kind: EffectKind) {
    return this.engine.perform(fxId(deck, slot, `.${kind}`), 1);
  }

  setMix(deck: number, slot: number, mix: number) {
    return this.engine.perform(fxId(deck, slot, '.mix'), mix);
  }

  setAmount(deck: number, slot: number, amount: number) {
    return this.engine.perform(fxId(deck, slot, '.amount'), amount);
  }

  setParam2(deck: number, slot: number, param2: number) {
    return this.engine.perform(fxId(deck, slot, '.param2'), param2);
  }

  /** The slot's period halved (-1) or doubled (1) so many times, from 1/8 beat to 8. */
  stepBeats(deck: number, slot: number, steps: number) {
    return this.engine.perform(`${fxId(deck, slot)}.beats`, steps, true);
  }

  /** The whole chain on or off, each slot keeping its own switch. */
  setChainEnabled(deck: number, enabled: boolean) {
    return this.engine.turn(`deck${deck + 1}.fx.chain`, enabled);
  }

  /** The chain on the next part it can be on, after the last on the whole deck again. */
  nextPart(deck: number) {
    return this.engine.press(deck, 'fx.part');
  }
}
