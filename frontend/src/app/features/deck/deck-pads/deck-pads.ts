import { Component, computed, inject, input, signal } from '@angular/core';
import { FxService } from '../../../core/services/fx.service';
import { SamplerService } from '../../../core/services/sampler.service';
import { EngineService } from '../../../core/services/engine.service';
import { PerformanceService } from '../../../core/services/performance.service';
import { EffectKind, HOT_CUES } from '../../../core/models/engine';
import { Knob } from '../../../shared/knob/knob';
import { formatBeats, loopBeats } from '../../../shared/beats';
import { t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';

type PadMode = 'cues' | 'loops' | 'roll' | 'jump' | 'fx' | 'sampler';

interface Pad {
  label: string;
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
  { id: 'fx', label: 'FX' },
  { id: 'sampler', label: 'SMPL' },
];
const SAMPLE_COLOR = 'var(--pr-tag-violet)';
/** The effects on the pads, with what their AMOUNT knob does. */
const EFFECTS: { kind: EffectKind; label: string; title: string }[] = [
  { kind: 'echo', label: 'ECHO', title: t('Echo on the beat · amount: feedback') },
  {
    kind: 'pingPong',
    label: 'PING',
    title: t('Echo bouncing left and right · amount: feedback'),
  },
  { kind: 'reverb', label: 'VERB', title: t('Reverb · amount: room size') },
  { kind: 'flanger', label: 'FLANGE', title: t('Flanger · amount: feedback') },
  { kind: 'phaser', label: 'PHASE', title: t('Phaser · amount: feedback') },
  {
    kind: 'filter',
    label: 'FILTER',
    title: t('Filter sweeping on the beat · amount: depth'),
  },
  { kind: 'gate', label: 'GATE', title: t('Cuts the sound on the beat · amount: depth') },
  { kind: 'bitcrusher', label: 'CRUSH', title: t('Lower resolution · amount: crush') },
];
const LOOP_BEATS = [0.25, 0.5, 1, 2, 4, 8, 16, 32];
const ROLL_BEATS = [1 / 16, 1 / 8, 1 / 4, 1 / 2, 1, 2, 4, 8];
const JUMP_BEATS = [-32, -16, -4, -1, 1, 4, 16, 32];

/**
 * Eight performance pads per deck (hot cues, loops, rolls, beat jumps, beat effects,
 * sampler), with the loop and slip keys or, for the effects, their level, knob and period.
 */
@Component({
  selector: 'app-deck-pads',
  templateUrl: './deck-pads.html',
  styleUrl: './deck-pads.scss',
  imports: [Knob, TPipe],
})
export class DeckPads {
  readonly deck = input.required<number>();

  private readonly engine = inject(EngineService);
  private readonly performance = inject(PerformanceService);
  protected readonly fx = inject(FxService);
  protected readonly sampler = inject(SamplerService);
  protected readonly modes = MODES;
  protected readonly mode = signal<PadMode>('cues');
  private readonly status = computed(() => this.engine.status()?.decks[this.deck()] ?? null);

  /** Loops and jumps need a beat grid, and a record in absolute mode keeps the position. */
  protected readonly canMove = computed(() => {
    const settings = this.engine.decks()[this.deck()];
    const absolute = settings.vinylEnabled && settings.vinylMode === 'absolute';
    return !!this.status()?.loaded && !absolute;
  });
  protected readonly loopActive = computed(() => this.status()?.looping.active ?? false);
  protected readonly loopInSet = computed(() => this.status()?.looping.loopInS != null);
  protected readonly slip = computed(() => this.status()?.slipMode ?? false);
  protected readonly effect = computed(() => this.engine.decks()[this.deck()].fx);
  protected readonly samplerGain = computed(() => this.engine.decks()[this.deck()].samplerGain);
  private readonly samples = computed(() => this.engine.decks()[this.deck()].samples);
  /** Effects and samples work on any deck; the rest needs a track that can move. */
  protected readonly padsEnabled = computed(
    () => this.mode() === 'fx' || this.mode() === 'sampler' || this.canMove(),
  );
  protected readonly formatBeats = formatBeats;

  private readonly activeLoopBeats = computed(() => {
    const looping = this.status()?.looping;
    const bpm = this.engine.tracks()[this.deck()]?.tempo?.bpm;
    if (!looping?.active || looping.startS === null || looping.endS === null || !bpm) return null;
    return loopBeats(looping.endS - looping.startS, 60 / bpm);
  });

  protected readonly pads = computed<Pad[]>(() => {
    switch (this.mode()) {
      case 'cues':
        return Array.from({ length: HOT_CUES }, (_, slot) => this.cuePad(slot));
      case 'loops':
        return LOOP_BEATS.map((b) =>
          this.beatPad(
            b,
            this.activeLoopBeats() === b,
            t('Loop {beats}', { beats: formatBeats(b) }),
          ),
        );
      case 'roll':
        return ROLL_BEATS.map((b) =>
          this.beatPad(b, false, t('Roll {beats} while held', { beats: formatBeats(b) })),
        );
      case 'jump':
        return JUMP_BEATS.map((b) =>
          this.beatPad(b, false, t('Jump {beats} beats', { beats: formatBeats(b) })),
        );
      case 'fx': {
        const fx = this.effect();
        return EFFECTS.map((e) => ({
          label: e.label,
          set: true,
          color: null,
          active: fx.enabled && fx.kind === e.kind,
          title: e.title,
        }));
      }
      case 'sampler':
        return this.samples().map((name, slot) => this.samplePad(name, slot));
    }
  });

  press(index: number, event: PointerEvent) {
    (event.currentTarget as Element).setPointerCapture(event.pointerId);
    const deck = this.deck();
    switch (this.mode()) {
      case 'cues':
        return event.shiftKey
          ? this.performance.deleteHotCue(deck, index)
          : this.performance.pressHotCue(deck, index);
      case 'loops':
        return this.activeLoopBeats() === LOOP_BEATS[index]
          ? this.performance.exitLoop(deck)
          : this.performance.beatLoop(deck, LOOP_BEATS[index]);
      case 'roll':
        return this.performance.loopRoll(deck, ROLL_BEATS[index]);
      case 'jump':
        return this.performance.beatJump(deck, JUMP_BEATS[index]);
      case 'fx':
        return this.fx.press(deck, EFFECTS[index].kind);
      case 'sampler':
        if (!this.samples()[index]) return this.sampler.choose(deck, index);
        return event.shiftKey ? this.sampler.clear(deck, index) : this.sampler.trigger(deck, index);
    }
  }

  release() {
    if (this.mode() === 'roll') this.performance.loopRoll(this.deck(), null);
  }

  /** Right click deletes a hot cue, stops a sound playing, or else takes it off its pad. */
  remove(index: number, event: MouseEvent) {
    event.preventDefault();
    const deck = this.deck();
    if (this.mode() === 'cues') this.performance.deleteHotCue(deck, index);
    if (this.mode() !== 'sampler') return;
    if (this.samplePlaying(index)) this.sampler.stop(deck, index);
    else if (this.samples()[index]) this.sampler.clear(deck, index);
  }

  loopIn() {
    this.performance.loopIn(this.deck());
  }

  loopOut() {
    this.performance.loopOut(this.deck());
  }

  scaleLoop(factor: number) {
    this.performance.scaleLoop(this.deck(), factor);
  }

  toggleLoop() {
    if (this.loopActive()) this.performance.exitLoop(this.deck());
    else this.performance.reloop(this.deck());
  }

  toggleSlip() {
    this.performance.setSlip(this.deck(), !this.slip());
  }

  censor(active: boolean) {
    this.performance.censor(this.deck(), active);
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

  private beatPad(beats: number, active: boolean, title: string): Pad {
    return { label: formatBeats(beats), set: true, color: null, active, title };
  }
}
