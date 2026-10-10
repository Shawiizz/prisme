import { Component, booleanAttribute, computed, input, linkedSignal, output } from '@angular/core';

/** The knob turns through 270 degrees, from bottom left to bottom right. */
const SWEEP = 270;
const START = -135;
/** Vertical drag for the whole travel (pixels); Shift is four times finer. */
const DRAG_PX = 160;
const WHEEL_STEP = 0.02;
const RADIUS = 9;

function point(angle: number): string {
  const rad = ((angle - 90) * Math.PI) / 180;
  return `${12 + RADIUS * Math.cos(rad)} ${12 + RADIUS * Math.sin(rad)}`;
}

function arc(from: number, to: number): string {
  if (Math.abs(to - from) < 0.5) return '';
  const [a, b] = from < to ? [from, to] : [to, from];
  return `M ${point(a)} A ${RADIUS} ${RADIUS} 0 ${b - a > 180 ? 1 : 0} 1 ${point(b)}`;
}

/**
 * Rotary knob: drag up or down (Shift for fine moves), scroll, double-click to go back
 * to its default. Its value goes from 0 to 1; a bipolar knob shows its travel from the
 * centre.
 */
@Component({
  selector: 'app-knob',
  template: `
    <svg
      viewBox="0 0 24 24"
      (pointerdown)="grab($event)"
      (pointermove)="drag($event)"
      (pointerup)="release($event)"
      (wheel)="scroll($event)"
      (dblclick)="valueChange.emit(defaultValue())"
    >
      <path class="track" [attr.d]="track" />
      <path class="value" [attr.d]="valueArc()" />
      <line x1="12" y1="12" [attr.x2]="tip().x" [attr.y2]="tip().y" />
    </svg>
    @if (label()) {
      <button
        class="label"
        [class.active]="active()"
        (click)="labelClick.emit()"
        [title]="labelTitle()"
      >
        {{ label() }}
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
      width: 24px;
      height: 24px;
      cursor: ns-resize;
      touch-action: none;
    }
    path {
      fill: none;
      stroke-width: 2.5;
      stroke-linecap: round;
    }
    .track {
      stroke: var(--line-strong);
    }
    .value {
      stroke: var(--accent, var(--text-2));
    }
    line {
      stroke: var(--text);
      stroke-width: 2;
      stroke-linecap: round;
    }
    .label {
      padding: 0 3px;
      border: none;
      background: transparent;
      color: var(--faint);
      font-size: 9px;
      font-weight: 700;
      letter-spacing: 0.04em;
      line-height: 11px;
      cursor: default;
      &.active {
        background: var(--red);
        color: var(--ink);
        border-radius: 2px;
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
  /** Highlights the label (a kill engaged, for instance). */
  readonly active = input(false);
  readonly valueChange = output<number>();
  readonly labelClick = output();

  protected readonly track = arc(START, START + SWEEP);
  /** Follows the input, and moves at once with the gestures (several can come before the
   * input does). */
  private readonly current = linkedSignal(() => this.value());
  private readonly angle = computed(() => START + Math.min(Math.max(this.current(), 0), 1) * SWEEP);
  protected readonly valueArc = computed(() =>
    arc(this.bipolar() ? START + SWEEP / 2 : START, this.angle()),
  );
  protected readonly tip = computed(() => {
    const rad = ((this.angle() - 90) * Math.PI) / 180;
    return { x: 12 + 6 * Math.cos(rad), y: 12 + 6 * Math.sin(rad) };
  });

  private grabbed: { y: number; value: number } | null = null;

  grab(event: PointerEvent) {
    (event.target as Element).setPointerCapture(event.pointerId);
    this.grabbed = { y: event.clientY, value: this.current() };
  }

  drag(event: PointerEvent) {
    if (!this.grabbed) return;
    const scale = event.shiftKey ? DRAG_PX * 4 : DRAG_PX;
    this.set(this.grabbed.value + (this.grabbed.y - event.clientY) / scale);
  }

  release(event: PointerEvent) {
    (event.target as Element).releasePointerCapture(event.pointerId);
    this.grabbed = null;
  }

  scroll(event: WheelEvent) {
    event.preventDefault();
    this.set(this.current() - Math.sign(event.deltaY) * WHEEL_STEP);
  }

  private set(value: number) {
    const clamped = Math.min(Math.max(value, 0), 1);
    if (clamped === this.current()) return;
    this.current.set(clamped);
    this.valueChange.emit(clamped);
  }
}
