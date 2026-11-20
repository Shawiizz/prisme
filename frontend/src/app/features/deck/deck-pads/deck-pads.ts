import { NgTemplateOutlet } from '@angular/common';
import { Component, DestroyRef, computed, inject, input, signal } from '@angular/core';
import { FxService } from '../../../core/services/fx.service';
import { SamplerService } from '../../../core/services/sampler.service';
import { EngineService } from '../../../core/services/engine.service';
import { PerformanceService } from '../../../core/services/performance.service';
import { EffectKind, FxSettings, PADS, SLICES } from '../../../core/models/engine';
import { Knob } from '../../../shared/knob/knob';
import { CUT_BEATS, ROLL_BEATS, formatBeats } from '../../../shared/beats';
import { readouts } from '../../../shared/mixer-scales';
import { Text, t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';
import { Press } from '../../../shared/press';
import { deckStems } from '../../../core/deck-stems';
import { EFFECT_NAMES, PARAM2 } from '../../../core/effects';

type PadMode = 'cues' | 'loops' | 'roll' | 'jump' | 'slice' | 'cut' | 'fx' | 'sampler';

interface Pad {
  label: string;
  /** Four letters in the place of the label on a pad too narrow for it. */
  short?: string;
  /** Something is on it: an empty pad (no cue, no sound) is dashed. */
  set: boolean;
  /** Tag colour of a hot cue or a sound. */
  color: string | null;
  active: boolean;
  title: string;
}

const MODES: { id: PadMode; label: string }[] = [
  { id: 'cues', label: 'CUES' },
  { id: 'loops', label: 'LOOP' },
  { id: 'roll', label: 'ROLL' },
  { id: 'jump', label: 'JUMP' },
  { id: 'slice', label: 'SLICE' },
  { id: 'cut', label: 'CUT' },
  { id: 'fx', label: 'FX' },
  { id: 'sampler', label: 'SMPL' },
];
const SAMPLE_COLOR = 'var(--pr-tag-violet)';
/** The bar of a loop or jump pad of the deck's size. */
const SIZE_COLOR = 'var(--pr-light)';
/** The bar of an effect pad whose effect sounds in another slot of the chain. */
const ELSEWHERE_COLOR = 'var(--pr-text-3)';
/** The effects on the pads, their name in four letters on a narrow pad, with what their AMOUNT
 * knob does. */
const EFFECTS: { kind: EffectKind; label: string; short: string; title: Text }[] = [
  {
    kind: 'echo',
    label: 'ECHO',
    short: 'ECHO',
    title: () => t('Echo on the beat · amount: feedback'),
  },
  {
    kind: 'pingPong',
    label: 'PING',
    short: 'PING',
    title: () => t('Ping-pong echo, bouncing left and right · amount: feedback'),
  },
  {
    kind: 'reverb',
    label: 'VERB',
    short: 'VERB',
    title: () => t('Reverb · amount: room size'),
  },
  {
    kind: 'flanger',
    label: 'FLANGE',
    short: 'FLNG',
    title: () => t('Flanger · amount: feedback'),
  },
  {
    kind: 'phaser',
    label: 'PHASE',
    short: 'PHSR',
    title: () => t('Phaser · amount: feedback'),
  },
  {
    kind: 'filter',
    label: 'FILTER',
    short: 'FLTR',
    title: () => t('Filter sweeping on the beat · amount: depth'),
  },
  {
    kind: 'gate',
    label: 'GATE',
    short: 'GATE',
    title: () => t('Gate, cutting the sound on the beat · amount: depth'),
  },
  {
    kind: 'bitcrusher',
    label: 'CRUSH',
    short: 'CRSH',
    title: () => t('Bitcrusher, lowering the resolution · amount: crush'),
  },
];
const LOOP_BEATS = [0.25, 0.5, 1, 2, 4, 8, 16, 32];
const JUMP_BEATS = [-32, -16, -4, -1, 1, 4, 16, 32];

/**
 * Eight performance pads per deck (hot cues and sounds on two pages), at the end of its rail (hot cues, loops, rolls, beat
 * jumps, the slicer, fader cuts, beat effects, sampler), with the keys that go with their mode: slip, censor
 * and reloop, all the loop keys, the deck's size for the jumps, the sampler's level, or the
 * slots of the effect chain with the level, knob and period of the one chosen.
 */
@Component({
  selector: 'app-deck-pads',
  templateUrl: './deck-pads.html',
  styleUrl: './deck-pads.scss',
  imports: [Knob, NgTemplateOutlet, Press, TPipe],
})
export class DeckPads {
  readonly deck = input.required<number>();

  private readonly engine = inject(EngineService);
  protected readonly performance = inject(PerformanceService);
  protected readonly fx = inject(FxService);
  protected readonly sampler = inject(SamplerService);
  protected readonly modes = MODES;
  protected readonly mode = signal<PadMode>('cues');
  /** The page of hot cues and sounds on the pads: 1 to 8, or 9 to 16. */
  protected readonly page = signal(0);
  /** Hot cues and sounds have two pages. */
  protected readonly paged = computed(() => this.mode() === 'cues' || this.mode() === 'sampler');
  private readonly first = computed(() => this.page() * PADS);
  /** The keys beside the pads. */
  protected readonly keys = computed(() => {
    const mode = this.mode();
    if (mode === 'loops' || mode === 'roll') return 'loop';
    if (mode === 'jump') return 'jump';
    return mode === 'fx' || mode === 'sampler' ? mode : 'slip';
  });
  private readonly status = computed(() => this.engine.status()?.decks[this.deck()] ?? null);

  /** Loops and jumps need a track, which a record in absolute mode keeps in place. */
  protected readonly canMove = computed(() => this.status()?.canMove ?? false);
  protected readonly loopActive = computed(() => this.status()?.looping.active ?? false);
  protected readonly loopInSet = computed(() => this.status()?.looping.loopInS != null);
  protected readonly slip = computed(() => this.status()?.slipMode ?? false);
  private readonly chain = computed(() => this.engine.decks()[this.deck()].fx);
  /** The slot of the chain the pads and knobs act on, and its effect. */
  protected readonly slot = computed(() => this.fx.chosen(this.deck()));
  protected readonly effect = computed(() => this.chain().slots[this.slot()]);
  /** A key per slot of the chain: chosen in the light, marked while its effect is on, framed
   * while a controller's effect knobs are focused on it. */
  protected readonly slotKeys = computed(() => {
    const focused = this.fx.focused(this.deck());
    return this.chain().slots.map((fx, slot) => {
      const title = fx.enabled
        ? t('Effect {n}: {effect}', { n: slot + 1, effect: EFFECT_NAMES[fx.kind]() })
        : t('Effect {n}: off', { n: slot + 1 });
      return {
        on: fx.enabled,
        focused: focused === slot,
        title: focused === slot ? t('{effect} · on the effect knobs', { effect: title }) : title,
      };
    });
  });
  /** The whole chain on: off, nothing sounds, each slot keeping its switch for later. */
  protected readonly chainOn = computed(() => this.chain().enabled);
  protected readonly chainTitle = computed(() =>
    this.chainOn()
      ? t('Effects on, each as its key says. Click to bypass them all')
      : t('Effects bypassed: none sounds. Click to hear them again'),
  );
  /** The second knob of the chosen slot's effect. */
  protected readonly param2 = computed(() => {
    const knob = PARAM2[this.effect().kind];
    return { label: knob.label, name: knob.name() };
  });
  private readonly stemKnobs = deckStems(this.deck).knobs;
  /** Where the effects are: the whole deck, or a part of the track as its stems call it. */
  protected readonly effectPart = computed(() => {
    const part = this.chain().part;
    const stem = this.stemKnobs().find((k) => k.part === part);
    return {
      on: part !== null,
      label: stem?.label ?? 'DECK',
      title: t('Effects on: {part}. Click for the next part', {
        part: stem ? stem.name : t('the whole deck'),
      }),
      choice: (this.engine.tracks()[this.deck()]?.separation ?? 'none') !== 'none',
    };
  });
  protected readonly samplerGain = computed(() => this.engine.decks()[this.deck()].samplerGain);
  private readonly samples = computed(() => this.engine.decks()[this.deck()].samples);
  /** Effects and samples work on any deck; the rest needs a track that can move. */
  protected readonly padsEnabled = computed(
    () => this.mode() === 'fx' || this.mode() === 'sampler' || this.canMove(),
  );
  /** The deck's size: what its jumps go by; the pads of that size have a bar in the light. */
  protected readonly size = computed(() => this.performance.sizes()[this.deck()]);
  protected readonly readouts = readouts;
  /** What lets go each pad held, by the pointer holding it: the action and deck it pressed,
   * even if the mode or the deck changed since. */
  private readonly held = new Map<number, () => void>();
  /** The deck CENSOR holds: the one let go, even if the float changed decks since. */
  private censored: number | null = null;

  constructor() {
    inject(DestroyRef).onDestroy(() => {
      for (const release of this.held.values()) release();
      this.held.clear();
    });
  }

  private readonly activeLoopBeats = computed(() => this.status()?.looping.beats ?? null);

  protected readonly pads = computed<Pad[]>(() => {
    switch (this.mode()) {
      case 'cues':
        return Array.from({ length: PADS }, (_, pad) => this.cuePad(this.first() + pad));
      case 'loops':
        return LOOP_BEATS.map((b) =>
          this.beatPad(
            b,
            this.activeLoopBeats() === b,
            t('Loop {beats}', { beats: formatBeats(b) }),
            b === this.size(),
          ),
        );
      case 'roll': {
        const playing = this.performance.rolls()[this.deck()];
        return ROLL_BEATS.map((b) =>
          this.beatPad(b, playing === b, t('Roll {beats} while held', { beats: formatBeats(b) })),
        );
      }
      case 'jump':
        return JUMP_BEATS.map((b) =>
          this.beatPad(
            b,
            false,
            t('Jump {beats} beats', { beats: formatBeats(b) }),
            Math.abs(b) === this.size(),
          ),
        );
      case 'slice': {
        const playing = this.status()?.lights.slice ?? null;
        return Array.from({ length: SLICES }, (_, slice) => ({
          label: `${slice + 1}`,
          set: true,
          color: null,
          active: playing === slice,
          title: t('Slice {n} while held', { n: slice + 1 }),
        }));
      }
      case 'cut': {
        const playing = this.performance.cuts()[this.deck()];
        return CUT_BEATS.map((b, i) =>
          this.beatPad(
            b,
            playing === i,
            t('Fader cut on {beats} while held', { beats: formatBeats(b) }),
          ),
        );
      }
      case 'fx': {
        const [slots, chosen] = [this.chain().slots, this.slot()];
        const sounds = (fx: FxSettings, kind: EffectKind) => fx.enabled && fx.kind === kind;
        return EFFECTS.map((e) => ({
          label: e.label,
          short: e.short,
          set: true,
          color: slots.some((fx, s) => s !== chosen && sounds(fx, e.kind)) ? ELSEWHERE_COLOR : null,
          active: sounds(slots[chosen], e.kind),
          title: e.title(),
        }));
      }
      case 'sampler':
        return Array.from({ length: PADS }, (_, pad) => {
          const slot = this.first() + pad;
          return this.samplePad(this.samples()[slot] ?? null, slot);
        });
    }
  });

  /** A length in beats, with its unit: under keys that halve and double it. */
  protected beats(beats: number): string {
    const count = formatBeats(beats);
    return Math.abs(beats) > 1
      ? t('{beats} beats', { beats: count })
      : t('{beats} beat', { beats: count });
  }

  press(index: number, event: PointerEvent) {
    // the right button is the pad's menu, not a press
    if (event.button !== 0) return;
    (event.currentTarget as Element).setPointerCapture(event.pointerId);
    const deck = this.deck();
    const slot = this.first() + index;
    switch (this.mode()) {
      case 'cues':
        return event.shiftKey
          ? this.performance.deleteHotCue(deck, slot)
          : this.performance.pressHotCue(deck, slot);
      case 'loops':
        // the pad of the loop playing leaves it
        return this.performance.beatLoop(deck, LOOP_BEATS[index]);
      case 'roll':
        this.performance.roll(deck, index, true);
        this.held.set(event.pointerId, () => this.performance.roll(deck, index, false));
        return;
      case 'jump':
        return this.performance.beatJump(deck, JUMP_BEATS[index]);
      case 'slice':
        this.performance.slice(deck, index, true);
        this.held.set(event.pointerId, () => this.performance.slice(deck, index, false));
        return;
      case 'cut':
        this.performance.cut(deck, index, true);
        this.held.set(event.pointerId, () => this.performance.cut(deck, index, false));
        return;
      case 'fx':
        return this.fx.press(deck, this.slot(), EFFECTS[index].kind);
      case 'sampler':
        if (!this.samples()[slot]) return this.sampler.choose(deck, slot);
        return event.shiftKey ? this.sampler.clear(deck, slot) : this.sampler.trigger(deck, slot);
    }
  }

  release(event: PointerEvent) {
    const release = this.held.get(event.pointerId);
    this.held.delete(event.pointerId);
    release?.();
  }

  /** The slicer is put on the bar the deck plays in as its mode is chosen. */
  choose(mode: PadMode) {
    this.mode.set(mode);
    if (mode === 'slice') this.performance.anchorSlicer(this.deck());
  }

  /** Right click deletes a hot cue, stops a sound playing, or else takes it off its pad. */
  remove(index: number, event: MouseEvent) {
    event.preventDefault();
    const deck = this.deck();
    const slot = this.first() + index;
    if (this.mode() === 'cues') this.performance.deleteHotCue(deck, slot);
    if (this.mode() !== 'sampler') return;
    if (this.samplePlaying(slot)) this.sampler.stop(deck, slot);
    else if (this.samples()[slot]) this.sampler.clear(deck, slot);
  }

  loopIn() {
    this.performance.loopIn(this.deck());
  }

  loopOut() {
    this.performance.loopOut(this.deck());
  }

  scaleLoop(factor: 0.5 | 2) {
    this.performance.scaleLoop(this.deck(), factor);
  }

  toggleLoop() {
    this.performance.reloop(this.deck());
  }

  toggleSlip() {
    this.performance.setSlip(this.deck(), !this.slip());
  }

  censor() {
    this.censored = this.deck();
    this.performance.censor(this.censored, true);
  }

  uncensor() {
    if (this.censored === null) return;
    this.performance.censor(this.censored, false);
    this.censored = null;
  }

  private cuePad(slot: number): Pad {
    const cue = this.performance.hotCue(this.deck(), slot);
    if (!cue)
      return {
        label: `${slot + 1}`,
        set: false,
        color: null,
        active: false,
        title: t('Click: set · Shift-click or right click: delete'),
      };
    const loop = cue.loopLengthS !== null;
    return {
      label: cue.label || `${slot + 1}${loop ? ' ↻' : ''}`,
      set: true,
      color: `#${cue.color.toString(16).padStart(6, '0')}`,
      active: false,
      title: loop
        ? t('Saved loop {n} · Shift-click or right click: delete', { n: slot + 1 })
        : t('Hot cue {n} · Shift-click or right click: delete', { n: slot + 1 }),
    };
  }

  private samplePad(name: string | null, slot: number): Pad {
    if (!name) {
      return {
        label: '+',
        set: false,
        color: null,
        active: false,
        title: t('Click: choose a sound'),
      };
    }
    return {
      label: name,
      set: true,
      color: SAMPLE_COLOR,
      active: this.samplePlaying(slot),
      title: t('{name} · Click: play · Right click: stop, then remove · Shift-click: remove', {
        name,
      }),
    };
  }

  private samplePlaying(slot: number): boolean {
    return ((this.status()?.samplesPlaying ?? 0) & (1 << slot)) !== 0;
  }

  /** A pad of so many beats; one of the deck's size has a bar in the light. */
  private beatPad(beats: number, active: boolean, title: string, sized = false): Pad {
    const color = sized ? SIZE_COLOR : null;
    return { label: formatBeats(beats), set: true, color, active, title };
  }
}
