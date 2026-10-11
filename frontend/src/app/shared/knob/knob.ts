import {
  Component,
  booleanAttribute,
  computed,
  input,
  linkedSignal,
  output,
  signal,
} from '@angular/core';

/** The knob turns through 270 degrees, from bottom left to bottom right. */
const SWEEP = 270;
const START = -135;
/** Vertical drag for the whole travel (pixels); Shift is four times finer. */
const DRAG_PX = 160;
const STEP = 0.02;
/** Geometry of the 36 × 36 drawing: the value ring around the cap. */
const CENTRE = 18;
const RING = 15;
const CAP = 11;

function polar(angle: number, radius: number): { x: number; y: number } {
  const rad = ((angle - 90) * Math.PI) / 180;
  return { x: CENTRE + radius * Math.cos(rad), y: CENTRE + radius * Math.sin(rad) };
}

function arc(from: number, to: number): string {
  if (Math.abs(to - from) < 0.5) return '';
  const [a, b] = from < to ? [from, to] : [to, from];
  const p = polar(a, RING);
  const q = polar(b, RING);
  return `M ${p.x} ${p.y} A ${RING} ${RING} 0 ${b - a > 180 ? 1 : 0} 1 ${q.x} ${q.y}`;
}

/**
 * Rotary knob: a cap with a line of light inside a ring that fills with the value, from the
 * centre for a bipolar knob (with a detent at the top). Drag up or down (Shift for fine
 * moves), scroll, use the arrow keys, double-click to go back to the default. While it is
 * touched, its label shows the value (`format`). Its value goes from 0 to 1.
 */
@Component({
  selector: 'app-knob',
  host: { '[class.small]': 'small()' },
  template: `
    <svg
      viewBox="0 0 36 36"
      role="slider"
      tabindex="0"
      aria-valuemin="0"
      aria-valuemax="1"
      [attr.aria-valuenow]="current()"
      [attr.aria-valuetext]="format()?.(current()) ?? null"
      [attr.aria-label]="labelTitle() || label()"
      [class.grabbed]="grabbed() !== null"
      (pointerdown)="grab($event)"
      (pointermove)="drag($event)"
      (pointerup)="release($event)"
      (pointercancel)="release($event)"
      (pointerenter)="hovered.set(true)"
      (pointerleave)="hovered.set(false)"
      (wheel)="scroll($event)"
      (keydown)="key($event)"
      (dblclick)="set(defaultValue())"
    >
      <path class="track" [attr.d]="track" />
      <path class="value" [attr.d]="valueArc()" />
      @if (bipolar()) {
        <line class="detent" [attr.x1]="CENTRE" y1="0.5" [attr.x2]="CENTRE" y2="2.5" />
      }
      <circle class="cap" [attr.cx]="CENTRE" [attr.cy]="CENTRE" [attr.r]="CAP" />
      <line
        class="pointer"
        [attr.x1]="pointer().from.x"
        [attr.y1]="pointer().from.y"
        [attr.x2]="pointer().to.x"
        [attr.y2]="pointer().to.y"
      />
    </svg>
    @if (label()) {
      <button
        class="label"
        [class.active]="active()"
        [class.reading]="reading()"
        (click)="labelClick.emit()"
        [title]="labelTitle()"
      >
        {{ reading() ? format()!(current()) : label() }}
      </button>
    }
  `,
  styles: `
    :host {
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: 1px;
    }
    svg {
      width: 34px;
      height: 34px;
      :host(.small) & {
        width: 26px;
        height: 26px;
      }
      cursor: ns-resize;
      touch-action: none;
      outline: none;
      &:focus-visible .cap {
        stroke: var(--pr-light);
      }
    }
    path {
      fill: none;
      stroke-width: 2.5;
      stroke-linecap: round;
    }
    .track {
      stroke: var(--pr-line-2);
    }
    .value {
      stroke: var(--pr-light);
    }
    .detent {
      stroke: var(--pr-text-2);
      stroke-width: 1.5;
    }
    .cap {
      fill: var(--pr-raise-2);
      stroke: var(--pr-line-2);
      stroke-width: 1;
      transition: stroke var(--pr-fast);
    }
    svg:hover .cap,
    svg.grabbed .cap {
      stroke: var(--pr-text-2);
    }
    .pointer {
      stroke: var(--pr-light);
      stroke-width: 2;
      stroke-linecap: round;
    }
    .label {
      min-width: 40px;
      height: auto;
      padding: 1px 4px;
      border: none;
      border-radius: var(--pr-radius-sm);
      background: transparent;
      color: var(--pr-text-3);
      font: 600 var(--pr-size-label) / 1.2 var(--pr-font);
      letter-spacing: 0.06em;
      text-align: center;
      white-space: nowrap;
      cursor: default;
      &.reading {
        color: var(--pr-text);
        font-family: var(--pr-font-mono);
        font-stretch: var(--pr-mono-width);
        letter-spacing: 0;
      }
      &.active {
        background: var(--pr-light);
        color: var(--pr-on-light);
      }
      &:hover {
        background: transparent;
      }
      &.active:hover {
        background: var(--pr-light);
      }
    }
    @media (prefers-reduced-motion: reduce) {
      .cap {
        transition: none;
      }
    }
  `,
})
export class Knob {
  readonly value = input.required<number>();
  readonly defaultValue = input(0.5);
  readonly bipolar = input(false, { transform: booleanAttribute });
  readonly label = input('');
  readonly labelTitle = input('');
  /** The value as read under the knob while it is touched (dB, %…). */
  readonly format = input<((value: number) => string) | null>(null);
  /** Highlights the label (a kill engaged, for instance). */
  readonly active = input(false);
  readonly small = input(false, { transform: booleanAttribute });
  readonly valueChange = output<number>();
  readonly labelClick = output();

  protected readonly CENTRE = CENTRE;
  protected readonly CAP = CAP;
  protected readonly track = arc(START, START + SWEEP);
  /** Follows the input, and moves at once with the gestures (several can come before the
   * input does). */
  protected readonly current = linkedSignal(() => this.value());
  private readonly angle = computed(() => START + Math.min(Math.max(this.current(), 0), 1) * SWEEP);
  protected readonly valueArc = computed(() =>
    arc(this.bipolar() ? START + SWEEP / 2 : START, this.angle()),
  );
  protected readonly pointer = computed(() => ({
    from: polar(this.angle(), 3),
    to: polar(this.angle(), CAP - 2.5),
  }));
  protected readonly hovered = signal(false);
  protected readonly grabbed = signal<{ y: number; value: number } | null>(null);
  protected readonly reading = computed(
    () => this.format() !== null && (this.hovered() || this.grabbed() !== null),
  );

  grab(event: PointerEvent) {
    (event.target as Element).setPointerCapture(event.pointerId);
    this.grabbed.set({ y: event.clientY, value: this.current() });
  }

  drag(event: PointerEvent) {
    const grabbed = this.grabbed();
    if (!grabbed) return;
    const scale = event.shiftKey ? DRAG_PX * 4 : DRAG_PX;
    this.set(grabbed.value + (grabbed.y - event.clientY) / scale);
  }

  release(event: PointerEvent) {
    (event.target as Element).releasePointerCapture(event.pointerId);
    this.grabbed.set(null);
  }

  scroll(event: WheelEvent) {
    event.preventDefault();
    this.set(this.current() - Math.sign(event.deltaY) * STEP);
  }

  key(event: KeyboardEvent) {
    const steps: Record<string, number> = {
      ArrowUp: STEP,
      ArrowRight: STEP,
      ArrowDown: -STEP,
      ArrowLeft: -STEP,
    };
    if (!(event.key in steps)) return;
    event.preventDefault();
    event.stopPropagation();
    this.set(this.current() + steps[event.key] * (event.shiftKey ? 0.25 : 1));
  }

  set(value: number) {
    const clamped = Math.min(Math.max(value, 0), 1);
    if (clamped === this.current()) return;
    this.current.set(clamped);
    this.valueChange.emit(clamped);
  }
}
