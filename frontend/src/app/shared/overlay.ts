import {
  DestroyRef,
  Directive,
  ElementRef,
  Injectable,
  effect,
  inject,
  output,
} from '@angular/core';
import { closedByBack } from '../core/back';
import { PHONE } from '../core/platform';

/** What is open over the page, the last opened on top: Escape closes the top one only, as a
 * controller's key does (`general.close`). */
@Injectable({ providedIn: 'root' })
export class OverlayService {
  private readonly open: { close: () => void }[] = [];

  constructor() {
    document.addEventListener('keydown', (event) => {
      if (event.key !== 'Escape' || !this.open.length) return;
      // the key is the overlay's: not a shortcut of the page under it
      event.stopPropagation();
      this.closeTop();
    });
  }

  /** Closes what is open on top, if anything is. */
  closeTop() {
    this.open.at(-1)?.close();
  }

  opened(close: () => void): () => void {
    const entry = { close };
    this.open.push(entry);
    return () => {
      const at = this.open.indexOf(entry);
      if (at >= 0) this.open.splice(at, 1);
    };
  }
}

export interface OverlayOptions {
  /** The back gesture of a phone closes it; `false` where what opens it answers the gesture. */
  back?: boolean;
}

/**
 * While `open`, what is open over the page closes with Escape and with the back gesture of a
 * phone, and once closed gives the keys back to what had them. To be called where `inject`
 * works; what is open as long as it exists passes `() => true`.
 */
export function overlay(
  open: () => boolean,
  close: () => void,
  { back = true }: OverlayOptions = {},
) {
  const overlays = inject(OverlayService);
  const host: HTMLElement = inject(ElementRef).nativeElement;
  if (PHONE && back) closedByBack(open, close);
  let closed: (() => void) | null = null;
  let opener: HTMLElement | null = null;
  const hide = () => {
    closed?.();
    closed = null;
    const focused = document.activeElement;
    const lost = !focused || focused === document.body || host.contains(focused);
    // a phone's keys are its fingers: a field given back the focus would raise the keyboard
    if (!PHONE && lost && opener?.isConnected) opener.focus({ preventScroll: true });
    opener = null;
  };
  effect(() => {
    if (open() && !closed) {
      closed = overlays.opened(close);
      const focused = document.activeElement;
      opener = focused instanceof HTMLElement && focused !== document.body ? focused : null;
    } else if (!open() && closed) {
      hide();
    }
  });
  inject(DestroyRef).onDestroy(() => {
    if (closed) hide();
  });
}

/**
 * A veil or a backdrop: `dismiss` when pressed and released on it, not on what it holds nor
 * at the end of a drag begun elsewhere (text selected in a field).
 */
@Directive({
  selector: '[dismiss]',
  host: {
    '(pointerdown)': 'pressed = on($event)',
    '(pointerup)': 'released = on($event)',
    '(click)': 'click($event)',
  },
})
export class Dismiss {
  readonly dismiss = output<void>();
  protected pressed = false;
  protected released = false;

  protected on(event: Event): boolean {
    return event.target === event.currentTarget;
  }

  protected click(event: MouseEvent) {
    if (this.pressed && this.released && this.on(event)) this.dismiss.emit();
    this.pressed = this.released = false;
  }
}
