import { Component, computed, inject, input, signal } from '@angular/core';
import { EngineService } from '../../../core/services/engine.service';
import { PerformanceService } from '../../../core/services/performance.service';
import { HOT_CUES } from '../../../core/models/engine';
import { deckColor } from '../../../shared/time';
import { formatBeats, loopBeats } from '../../../shared/beats';

type PadMode = 'cues' | 'loops' | 'roll' | 'jump';

interface Pad {
  label: string;
  /** CSS colour of a set hot cue. */
  color: string | null;
  active: boolean;
  title: string;
}

const MODES: { id: PadMode; label: string }[] = [
  { id: 'cues', label: 'CUES' },
  { id: 'loops', label: 'LOOP' },
  { id: 'roll', label: 'ROLL' },
  { id: 'jump', label: 'JUMP' },
];
const LOOP_BEATS = [0.25, 0.5, 1, 2, 4, 8, 16, 32];
const ROLL_BEATS = [1 / 16, 1 / 8, 1 / 4, 1 / 2, 1, 2, 4, 8];
const JUMP_BEATS = [-32, -16, -4, -1, 1, 4, 16, 32];

/** Eight performance pads per deck (hot cues, loops, rolls, beat jumps), loop and slip keys. */
@Component({
  selector: 'app-deck-pads',
  templateUrl: './deck-pads.html',
  styleUrl: './deck-pads.scss',
})
export class DeckPads {
  readonly deck = input.required<number>();

  private readonly engine = inject(EngineService);
  private readonly performance = inject(PerformanceService);
  protected readonly modes = MODES;
  protected readonly mode = signal<PadMode>('cues');
  protected readonly color = computed(() => deckColor(this.deck()));
  private readonly status = computed(() => this.engine.status()?.decks[this.deck()] ?? null);

  /** Loops and jumps need a beat grid, and a record in absolute mode keeps the position. */
  protected readonly canMove = computed(() => {
    const settings = this.engine.decks()[this.deck()];
    const absolute = settings.vinylEnabled && settings.vinylMode === 'absolute';
    return !!this.status()?.loaded && !absolute;
  });
  protected readonly loopActive = computed(() => this.status()?.looping.active ?? false);
  protected readonly slip = computed(() => this.status()?.slipMode ?? false);

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
          this.beatPad(b, this.activeLoopBeats() === b, `Loop ${formatBeats(b)}`),
        );
      case 'roll':
        return ROLL_BEATS.map((b) => this.beatPad(b, false, `Roll ${formatBeats(b)} while held`));
      case 'jump':
        return JUMP_BEATS.map((b) => this.beatPad(b, false, `Jump ${formatBeats(b)} beats`));
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
    }
  }

  release() {
    if (this.mode() === 'roll') this.performance.loopRoll(this.deck(), null);
  }

  /** Right click on a hot cue deletes it. */
  remove(index: number, event: MouseEvent) {
    event.preventDefault();
    if (this.mode() === 'cues') this.performance.deleteHotCue(this.deck(), index);
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
        color: null,
        active: false,
        title: 'Click: set · Shift-click or right click: delete',
      };
    const loop = cue.loopLengthS !== null;
    return {
      label: cue.label || `${slot + 1}${loop ? ' ↻' : ''}`,
      color: `#${cue.color.toString(16).padStart(6, '0')}`,
      active: false,
      title: `${loop ? 'Saved loop' : 'Hot cue'} ${slot + 1} · Shift-click or right click: delete`,
    };
  }

  private beatPad(beats: number, active: boolean, title: string): Pad {
    return { label: formatBeats(beats), color: null, active, title };
  }
}
