import { DestroyRef, Injectable, effect, inject } from '@angular/core';

/**
 * The back gesture of a phone closes what was opened last over the screen: each thing
 * opened is a step of the page's history, which the gesture walks back.
 */
@Injectable({ providedIn: 'root' })
export class BackService {
  private readonly closers: (() => void)[] = [];
  /** Steps back taken here, for what was closed another way: not the DJ's gesture. */
  private own = 0;

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
      this.own++;
      history.back();
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
