import {
  Component,
  ElementRef,
  booleanAttribute,
  computed,
  inject,
  input,
  linkedSignal,
  output,
} from '@angular/core';

/** Size of the cap along the travel (pixels): the track runs between its two centres. */
const CAP = 14;
/** Shift: a gesture moves the fader four times less. */
const FINE = 0.25;
/** Steps of the keyboard and the wheel, as a share of the travel. */
const STEP = 0.01;
const PAGE = 0.1;

/**
 * A fader like a mixer's: a thin track, a cap with a line of light, the travel filled from
 * the start or, when `centered`, from the middle where it has a detent. Drag it (Shift for
 * fine moves), click the track to jump, scroll, use the arrow keys, double-click to go back
 * to `defaultValue`.
 */
@Component({
  selector: 'app-fader',
  template: `
    <div class="track">
      <div class="fill" [style.--from]="fillFrom()" [style.--to]="fillTo()"></div>
      @if (centered()) {
        <i class="detent"></i>
      }
    </div>
    <div class="cap" [style.--at]="fraction()"><i></i></div>
  `,
  styles: `
    :host {
      position: relative;
      display: block;
      min-width: 0;
      min-height: 0;
      touch-action: none;
      cursor: pointer;
      outline: none;
      --cap: 14px; /* CAP */
    }
    :host(:not(.vertical)) {
      height: 24px;
    }
    :host(.vertical) {
      width: 24px;
    }
    :host(:focus-visible) .cap {
      outline: 2px solid var(--pr-light);
      outline-offset: 2px;
    }
    :host(.disabled) {
      cursor: default;
      .cap {
        background: var(--pr-base);
        i {
          background: var(--pr-text-3);
        }
      }
    }
    .track {
      position: absolute;
      border-radius: 2px;
      background: var(--pr-line-2);
    }
    :host(:not(.vertical)) .track {
      left: calc(var(--cap) / 2);
      right: calc(var(--cap) / 2);
      top: calc(50% - 2px);
      height: 4px;
    }
    :host(.vertical) .track {
      top: calc(var(--cap) / 2);
      bottom: calc(var(--cap) / 2);
      left: calc(50% - 2px);
      width: 4px;
    }
    .fill {
      position: absolute;
      border-radius: 2px;
      background: var(--pr-text-3);
    }
    :host(:not(.vertical)) .fill {
      top: 0;
      bottom: 0;
      left: calc(var(--from) * 100%);
      right: calc((1 - var(--to)) * 100%);
    }
    :host(.vertical) .fill {
      left: 0;
      right: 0;
      bottom: calc(var(--from) * 100%);
      top: calc((1 - var(--to)) * 100%);
    }
    .detent {
      position: absolute;
      background: var(--pr-text-2);
    }
    :host(:not(.vertical)) .detent {
      left: calc(50% - 0.5px);
      top: -5px;
      width: 1px;
      height: 14px;
    }
    :host(.vertical) .detent {
      top: calc(50% - 0.5px);
      left: -5px;
      height: 1px;
      width: 14px;
    }
    .cap {
      position: absolute;
      display: grid;
      place-items: center;
      border: 1px solid var(--pr-line-2);
      border-radius: 3px;
      background: var(--pr-raise-2);
      transition: border-color var(--pr-fast);
      i {
        display: block;
        background: var(--pr-light);
        border-radius: 1px;
      }
    }
    :host(:hover:not(.disabled)) .cap,
    :host(.grabbed) .cap {
      border-color: var(--pr-text-2);
    }
    :host(:not(.vertical)) .cap {
      top: 0;
      bottom: 0;
      width: var(--cap);
      left: calc(var(--at) * (100% - var(--cap)));
      i {
        width: 2px;
        height: 12px;
      }
    }
    :host(.vertical) .cap {
      left: 0;
      right: 0;
      height: var(--cap);
      bottom: calc(var(--at) * (100% - var(--cap)));
      i {
        height: 2px;
        width: 12px;
      }
    }
    @media (prefers-reduced-motion: reduce) {
      .cap {
        transition: none;
      }
    }
  `,
  host: {
    role: 'slider',
    '[attr.tabindex]': 'disabled() ? -1 : 0',
    '[attr.aria-label]': 'label() || null',
    '[attr.aria-orientation]': 'orientation()',
    '[attr.aria-valuemin]': 'min()',
    '[attr.aria-valuemax]': 'max()',
    '[attr.aria-valuenow]': 'current()',
    '[attr.aria-disabled]': 'disabled()',
    '[class.vertical]': "orientation() === 'vertical'",
    '[class.disabled]': 'disabled()',
    '[class.grabbed]': 'grabbed !== null',
    '(pointerdown)': 'grab($event)',
    '(pointermove)': 'drag($event)',
    '(pointerup)': 'release($event)',
    '(pointercancel)': 'release($event)',
    '(dblclick)': 'set(defaultValue())',
    '(wheel)': 'scroll($event)',
    '(keydown)': 'key($event)',
  },
})
export class Fader {
  readonly value = input.required<number>();
  readonly min = input(0);
  readonly max = input(1);
  readonly defaultValue = input(0);
  /** Filled from the middle, with a detent there (pitch, crossfader). */
  readonly centered = input(false, { transform: booleanAttribute });
  readonly orientation = input<'horizontal' | 'vertical'>('horizontal');
  readonly disabled = input(false, { transform: booleanAttribute });
  readonly label = input('');
  readonly valueChange = output<number>();

  private readonly host = inject(ElementRef<HTMLElement>);
  /** Follows the input, and moves at once with the gestures. */
  protected readonly current = linkedSignal(() => this.value());
  protected readonly fraction = computed(() => this.toFraction(this.current()));
  private readonly origin = computed(() => (this.centered() ? 0.5 : 0));
  protected readonly fillFrom = computed(() => Math.min(this.origin(), this.fraction()));
  protected readonly fillTo = computed(() => Math.max(this.origin(), this.fraction()));

  protected grabbed: { pointer: number; fraction: number } | null = null;

  grab(event: PointerEvent) {
    if (this.disabled() || event.button !== 0) return;
    this.host.nativeElement.setPointerCapture(event.pointerId);
    const onCap = (event.target as Element).closest('.cap') !== null;
    const pointer = this.pointerFraction(event);
    // grabbing the cap keeps it under the pointer; the track makes it jump there
    if (!onCap) this.setFraction(pointer);
    this.grabbed = { pointer, fraction: onCap ? this.fraction() : pointer };
  }

  drag(event: PointerEvent) {
    if (!this.grabbed) return;
    const moved =
      (this.pointerFraction(event) - this.grabbed.pointer) * (event.shiftKey ? FINE : 1);
    this.setFraction(this.grabbed.fraction + moved);
  }

  release(event: PointerEvent) {
    if (!this.grabbed) return;
    this.host.nativeElement.releasePointerCapture(event.pointerId);
    this.grabbed = null;
  }

  scroll(event: WheelEvent) {
    if (this.disabled()) return;
    event.preventDefault();
    this.setFraction(this.fraction() - Math.sign(event.deltaY) * STEP);
  }

  key(event: KeyboardEvent) {
    if (this.disabled()) return;
    const steps: Record<string, number> = {
      ArrowUp: STEP,
      ArrowRight: STEP,
      ArrowDown: -STEP,
      ArrowLeft: -STEP,
      PageUp: PAGE,
      PageDown: -PAGE,
    };
    if (event.key in steps) {
      this.setFraction(this.fraction() + steps[event.key] * (event.shiftKey ? FINE : 1));
    } else if (event.key === 'Home') {
      this.setFraction(0);
    } else if (event.key === 'End') {
      this.setFraction(1);
    } else {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
  }

  set(value: number) {
    if (this.disabled()) return;
    this.setFraction(this.toFraction(value));
  }

  private setFraction(fraction: number) {
    const f = Math.min(Math.max(fraction, 0), 1);
    const value = this.min() + f * (this.max() - this.min());
    if (value === this.current()) return;
    this.current.set(value);
    this.valueChange.emit(value);
  }

  private toFraction(value: number): number {
    const span = this.max() - this.min();
    return span > 0 ? Math.min(Math.max((value - this.min()) / span, 0), 1) : 0;
  }

  /** Where the pointer is along the travel, 0 at the start (left, or bottom). */
  private pointerFraction(event: PointerEvent): number {
    const rect = this.host.nativeElement.getBoundingClientRect();
    return this.orientation() === 'vertical'
      ? (rect.bottom - CAP / 2 - event.clientY) / (rect.height - CAP)
      : (event.clientX - rect.left - CAP / 2) / (rect.width - CAP);
  }
}
