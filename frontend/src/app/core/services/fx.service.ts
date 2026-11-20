import { Injectable, effect, inject, signal, untracked } from '@angular/core';
import {
  EFFECT_KINDS,
  EffectKind,
  FX_BEATS,
  FxChain,
  FxSettings,
  MAX_DECKS,
} from '../models/engine';
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
  private readonly focusedSlots = signal<(number | null)[]>(Array(MAX_DECKS).fill(null));

  constructor() {
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

  /** Focuses the slot, chosen on the screen too; pressed again, lets it go. */
  toggleFocus(deck: number, slot: number) {
    const focused = this.focused(deck) === slot ? null : slot;
    this.focusedSlots.update((slots) => slots.map((s, d) => (d === deck ? focused : s)));
    if (focused !== null) this.choose(deck, focused);
  }

  /** Puts `kind` in the slot and switches it on; pressed again, switches it off. */
  press(deck: number, slot: number, kind: EffectKind) {
    const fx = this.settings(deck, slot);
    if (fx.kind === kind && fx.enabled) return this.setEnabled(deck, slot, false);
    if (fx.kind !== kind) this.setKind(deck, slot, kind);
    return this.setEnabled(deck, slot, true);
  }

  /** The effect `step` places after the slot's (before it, negative), on or off as it was. */
  cycle(deck: number, slot: number, step: number) {
    const at = EFFECT_KINDS.indexOf(this.settings(deck, slot).kind);
    const kind = EFFECT_KINDS[(at + step + EFFECT_KINDS.length * 2) % EFFECT_KINDS.length];
    return this.setKind(deck, slot, kind);
  }

  /** Another effect in the slot, on or off as it was. */
  setKind(deck: number, slot: number, kind: EffectKind) {
    if (this.settings(deck, slot).kind === kind) return Promise.resolve();
    this.patch(deck, slot, { kind });
    return this.engine.call('set_fx', { deck, slot, kind });
  }

  setEnabled(deck: number, slot: number, enabled: boolean) {
    this.patch(deck, slot, { enabled });
    return this.engine.call('set_fx_enabled', { deck, slot, enabled });
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

  /** Halves (0.5) or doubles (2) the slot's period. */
  scaleBeats(deck: number, slot: number, factor: number) {
    return this.setBeats(deck, slot, this.settings(deck, slot).beats * factor);
  }

  setBeats(deck: number, slot: number, beats: number) {
    beats = Math.min(Math.max(beats, FX_BEATS.min), FX_BEATS.max);
    this.patch(deck, slot, { beats });
    return this.engine.call('set_fx_beats', { deck, slot, beats });
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
    const parts = this.parts(deck);
    const at = parts.indexOf(this.chain(deck).part);
    return this.setPart(deck, parts[(at + 1) % parts.length]);
  }

  private patch(deck: number, slot: number, changes: Partial<FxSettings>) {
    const chain = this.chain(deck);
    const slots = chain.slots.map((fx, s) => (s === slot ? { ...fx, ...changes } : fx));
    this.engine.patchDeck(deck, { fx: { ...chain, slots } });
  }
}
