import { DestroyRef, Injectable, effect, inject } from '@angular/core';

/**
 * The back gesture of a phone closes what was opened last over the screen: each thing
 * opened is a step of the page's history, which the gesture walks back.
 */
@Injectable({ providedIn: 'root' })
export class BackService {
  private readonly closers: (() => void)[] = [];
  /** Walks back taken here, for what was closed another way: not the DJ's gesture. */
  private own = 0;
  /** Steps of what was closed another way in this task, walked back at once at its end: a
   * WebView may merge several walks into one, and the DJ's next gesture would be lost. */
  private closing = 0;

  constructor() {
    addEventListener('popstate', () => {
      if (this.own > 0) this.own--;
      else this.closers.pop()?.();
    });
  }

  /** `close` is what the gesture does next; the call returned tells it was closed otherwise. */
  opened(close: () => void): () => void {
    this.closers.push(close);
    history.pushState(null, '');
    return () => {
      const at = this.closers.indexOf(close);
      if (at < 0) return;
      this.closers.splice(at, 1);
      if (this.closing++ > 0) return;
      queueMicrotask(() => {
        const steps = this.closing;
        this.closing = 0;
        this.own++;
        history.go(-steps);
      });
    };
  }
}

/** While `open`, the back gesture calls `close`. To be called where `inject` works. */
export function closedByBack(open: () => boolean, close: () => void) {
  const back = inject(BackService);
  let closedOtherwise: (() => void) | null = null;
  effect(() => {
    if (open() && !closedOtherwise) {
      closedOtherwise = back.opened(() => {
        closedOtherwise = null;
        close();
      });
    } else if (!open() && closedOtherwise) {
      closedOtherwise();
      closedOtherwise = null;
    }
  });
  inject(DestroyRef).onDestroy(() => closedOtherwise?.());
}
