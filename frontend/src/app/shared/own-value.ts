import { DestroyRef, Signal, WritableSignal, inject, linkedSignal, untracked } from '@angular/core';

/** How long after its last gesture a control keeps its own value over the backend's (ms): the
 * echo of its value is a frame or two away, echoes of older values may come before it. */
export const SETTLE_MS = 250;

/** A control's value, and what tells it a gesture moved it or let go of it. */
export interface OwnValue {
  readonly current: WritableSignal<number>;
  moved(): void;
}

/**
 * A control's value: the backend's (`value`), but its own while it is `held` (grabbed) and for
 * a moment after each gesture, the backend's echoes of older values coming in between; the
 * backend's again once that moment is over. Made in an injection context.
 */
export function ownValue(value: Signal<number>, held: () => boolean): OwnValue {
  let moving = false;
  let settling: ReturnType<typeof setTimeout> | undefined;
  const current = linkedSignal<number, number>({
    source: value,
    computation: (now, before) => (before && (moving || untracked(held)) ? before.value : now),
  });
  inject(DestroyRef).onDestroy(() => clearTimeout(settling));
  return {
    current,
    moved() {
      moving = true;
      clearTimeout(settling);
      settling = setTimeout(() => {
        moving = false;
        if (!untracked(held)) current.set(untracked(value));
      }, SETTLE_MS);
    },
  };
}
