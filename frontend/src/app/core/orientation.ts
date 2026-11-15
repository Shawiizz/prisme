import { DestroyRef, Signal, inject, signal } from '@angular/core';

/** Lower than this, a screen leaves each deck a line over its waveform: a phone held sideways,
 * not a tablet. */
const SHORT_PX = 500;

/** Whether the screen is wider than tall: a phone or a tablet held sideways, a window lying
 * down. To be called where `inject` works. */
export function landscape(): Signal<boolean> {
  return matching('(orientation: landscape)');
}

/** Whether the screen is too low for a deck's readouts under its waveform. To be called where
 * `inject` works. */
export function short(): Signal<boolean> {
  return matching(`(max-height: ${SHORT_PX}px)`);
}

/** Whether the screen matches the media `query`, as it turns or changes size. */
function matching(query: string): Signal<boolean> {
  const list = matchMedia(query);
  const matches = signal(list.matches);
  const follow = () => matches.set(list.matches);
  list.addEventListener('change', follow);
  inject(DestroyRef).onDestroy(() => list.removeEventListener('change', follow));
  return matches.asReadonly();
}
