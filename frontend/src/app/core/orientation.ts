import { DestroyRef, Signal, inject, signal } from '@angular/core';

/** Whether the screen is wider than tall: a phone held sideways. To be called where `inject` works. */
export function landscape(): Signal<boolean> {
  const query = matchMedia('(orientation: landscape)');
  const wide = signal(query.matches);
  const follow = () => wide.set(query.matches);
  query.addEventListener('change', follow);
  inject(DestroyRef).onDestroy(() => query.removeEventListener('change', follow));
  return wide.asReadonly();
}
