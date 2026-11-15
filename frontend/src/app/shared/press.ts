import { Directive, OnDestroy, output } from '@angular/core';

/**
 * A key that answers as it is pressed, as on a deck, rather than on its click: `press` when
 * the main button or a finger goes down on it, `release` when it comes up, even off the key.
 * `release` tells whether the key was let go (`true`) or the press lost: cancelled by the
 * system, or the key gone from under it. From the keyboard (a click no pointer made), both
 * at once.
 */
@Directive({
  selector: '[press]',
  host: {
    '(pointerdown)': 'down($event)',
    '(pointerup)': 'up($event, true)',
    '(pointercancel)': 'up($event, false)',
    '(click)': 'key($event)',
  },
})
export class Press implements OnDestroy {
  readonly press = output<void>();
  readonly release = output<boolean>();
  /** The pointer holding the key. */
  private held: number | null = null;

  protected down(event: PointerEvent) {
    if (event.button !== 0 || this.held !== null) return;
    (event.currentTarget as Element).setPointerCapture(event.pointerId);
    this.held = event.pointerId;
    this.press.emit();
  }

  protected up(event: PointerEvent, letGo: boolean) {
    if (event.pointerId !== this.held) return;
    this.held = null;
    this.release.emit(letGo);
  }

  protected key(event: MouseEvent) {
    if (event.detail !== 0) return;
    this.press.emit();
    this.release.emit(true);
  }

  // a hook rather than DestroyRef: it runs while the key's listeners still hear it
  ngOnDestroy() {
    if (this.held === null) return;
    this.held = null;
    this.release.emit(false);
  }
}
