import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';
import { EffectKind, FxChain, FxSettings, MAX_DECKS } from '../models/engine';
import { fxId } from '../control/controls';
import { StemPart, hasPart } from '../stem-parts';
import { EngineService } from './engine.service';

/**
 * Each deck's chain of beat effects: in each slot, which effect, on or off, its level, its
 * two knobs and its period; the whole chain on or off; the part of the track the chain is
 * on; the slot the pads and knobs of the screen act on, and the slot a controller's knobs
 * are focused on, if any. A track loaded without that part puts the chain back on the whole
 * deck, where it is heard.
 */
@Injectable({ providedIn: 'root' })
export class FxService {
  private readonly engine = inject(EngineService);
  private readonly chosenSlots = signal<number[]>(Array(MAX_DECKS).fill(0));
  /** The slot each deck's effect knobs are focused on, as the backend keeps it. */
  private readonly focusedSlots = computed<(number | null)[]>(
    () => this.engine.controls()?.perform.map((d) => d.fxFocus) ?? Array(MAX_DECKS).fill(null),
    { equal: (a, b) => a.every((slot, deck) => slot === b[deck]) },
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
    effect(() => {
      const tracks = this.engine.tracks();
      untracked(() => {
        tracks.forEach((track, deck) => {
          const part = this.chain(deck).part;
          if (track && part && !hasPart(track.separation, part)) void this.setPart(deck, null);
        });
      });
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
    this.patch(deck, slot, { mix });
    return this.engine.call('set_fx_mix', { deck, slot, mix });
  }

  setAmount(deck: number, slot: number, amount: number) {
    this.patch(deck, slot, { amount });
    return this.engine.call('set_fx_amount', { deck, slot, amount });
  }

  setParam2(deck: number, slot: number, param2: number) {
    this.patch(deck, slot, { param2 });
    return this.engine.call('set_fx_param2', { deck, slot, param2 });
  }

  /** The slot's period halved (-1) or doubled (1) so many times, from 1/8 beat to 8. */
  stepBeats(deck: number, slot: number, steps: number) {
    return this.engine.perform(`${fxId(deck, slot)}.beats`, steps, true);
  }

  /** The whole chain on or off, each slot keeping its own switch. */
  setChainEnabled(deck: number, enabled: boolean) {
    this.engine.patchDeck(deck, { fx: { ...this.chain(deck), enabled } });
    return this.engine.call('set_fx_chain_enabled', { deck, enabled });
  }

  /** The parts the chain can be on alone, as the deck's track is separated, after the
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
    this.engine.patchDeck(deck, { fx: { ...this.chain(deck), part } });
    return this.engine.call('set_fx_part', { deck, part });
  }

  /** The chain on the next part it can be on, after the last on the whole deck again. */
  nextPart(deck: number) {
    return this.engine.press(deck, 'fx.part');
  }

  private patch(deck: number, slot: number, changes: Partial<FxSettings>) {
    const chain = this.chain(deck);
    const slots = chain.slots.map((fx, s) => (s === slot ? { ...fx, ...changes } : fx));
    this.engine.patchDeck(deck, { fx: { ...chain, slots } });
  }
}
