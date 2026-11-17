import { DestroyRef, Directive, ElementRef, inject, output } from '@angular/core';

/** Pulled down further than this share of its height, a sheet closes when let go. */
const CLOSE_SHARE = 0.3;
/** Let go moving down faster than this (px/ms), a sheet closes however far it was pulled. */
const FLING = 0.5;
/** A finger moving this far (px) has chosen between pulling the sheet and scrolling it. */
const DECIDE_PX = 6;
const SLIDE_MS = 220;
const EASE = 'cubic-bezier(0.2, 0.8, 0.2, 1)';
/** Where a finger moves a setting, not the sheet. */
const KEEPS_THE_FINGER =
  'input, select, textarea, [role="slider"], app-knob, app-fader, app-crossfader';

/**
 * A sheet risen from the bottom of a phone's screen. A finger pulls it down from its top, or
 * from anywhere once its content is scrolled to the top, and lets go of it to close it or have
 * it spring back; closed (`close`), it slides down before it goes. The veil behind it fades
 * with it. `dismissed` once it is out of sight.
 */
@Directive({ selector: '[prSheet]', exportAs: 'prSheet' })
export class Sheet {
  readonly dismissed = output();

  private readonly sheet: HTMLElement = inject(ElementRef).nativeElement;
  private pull: {
    startY: number;
    startX: number;
    lastY: number;
    lastAt: number;
    speed: number;
    on: boolean;
  } | null = null;
  private closing = false;
  private gone = false;

  constructor() {
    const touched = (event: TouchEvent) => this.touched(event);
    const moved = (event: TouchEvent) => this.moved(event);
    const released = () => this.released();
    this.sheet.addEventListener('touchstart', touched, { passive: true });
    this.sheet.addEventListener('touchmove', moved, { passive: false });
    this.sheet.addEventListener('touchend', released);
    this.sheet.addEventListener('touchcancel', released);
    inject(DestroyRef).onDestroy(() => {
      this.gone = true;
      this.sheet.removeEventListener('touchstart', touched);
      this.sheet.removeEventListener('touchmove', moved);
      this.sheet.removeEventListener('touchend', released);
      this.sheet.removeEventListener('touchcancel', released);
    });
  }

  /** Slides the sheet down and its veil out, then tells it is gone. */
  close() {
    if (this.closing) return;
    this.closing = true;
    if (!this.risen() || matchMedia('(prefers-reduced-motion: reduce)').matches) {
      this.dismissed.emit();
      return;
    }
    this.slide(this.sheet.offsetHeight, () => {
      if (!this.gone) this.dismissed.emit();
    });
  }

  private touched(event: TouchEvent) {
    const target = event.target as Element;
    if (
      this.closing ||
      !this.risen() ||
      event.touches.length !== 1 ||
      target.closest(KEEPS_THE_FINGER)
    ) {
      return;
    }
    const { clientX, clientY } = event.touches[0];
    this.pull = {
      startY: clientY,
      startX: clientX,
      lastY: clientY,
      lastAt: event.timeStamp,
      speed: 0,
      on: false,
    };
  }

  private moved(event: TouchEvent) {
    const pull = this.pull;
    if (!pull) return;
    const { clientX, clientY } = event.touches[0];
    const down = clientY - pull.startY;
    if (!pull.on) {
      if (Math.max(Math.abs(down), Math.abs(clientX - pull.startX)) < DECIDE_PX) return;
      // down, at the top of its content and more down than sideways: the sheet's; else a scroll
      if (down <= 0 || this.sheet.scrollTop > 0 || Math.abs(clientX - pull.startX) > down) {
        this.pull = null;
        return;
      }
      pull.on = true;
      this.sheet.style.transition = 'none';
    }
    event.preventDefault();
    const elapsed = event.timeStamp - pull.lastAt;
    if (elapsed > 0) pull.speed = (clientY - pull.lastY) / elapsed;
    pull.lastY = clientY;
    pull.lastAt = event.timeStamp;
    this.place(Math.max(down, 0));
  }

  private released() {
    const pull = this.pull;
    this.pull = null;
    if (!pull?.on) return;
    const down = Math.max(pull.lastY - pull.startY, 0);
    if (down > this.sheet.offsetHeight * CLOSE_SHARE || pull.speed > FLING) this.close();
    else this.slide(0);
  }

  /** The sheet `down` px below its place, its veil as faded. */
  private place(down: number) {
    this.sheet.style.transform = down ? `translateY(${down}px)` : '';
    const veil = this.veil();
    if (veil) veil.style.opacity = String(1 - down / Math.max(this.sheet.offsetHeight, 1));
  }

  /** Slides the sheet to `down` px below its place, then `done`. */
  private slide(down: number, done?: () => void) {
    const veil = this.veil();
    const transition = `transform ${SLIDE_MS}ms ${EASE}`;
    this.sheet.style.transition = transition;
    if (veil) veil.style.transition = `opacity ${SLIDE_MS}ms ${EASE}`;
    this.place(down);
    if (done) setTimeout(done, SLIDE_MS);
  }

  /** Risen from the bottom of the screen: what is a sheet on a phone floats elsewhere. */
  private risen(): boolean {
    return this.sheet.classList.contains('pr-sheet');
  }

  /** The veil laid behind the sheet, just before it. */
  private veil(): HTMLElement | null {
    const before = this.sheet.previousElementSibling;
    return before instanceof HTMLElement && before.classList.contains('pr-veil') ? before : null;
  }
}
