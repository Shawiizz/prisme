/**
 * Follows a press on a handle that resizes something: from its size `from`, the size grows as
 * the pointer moves along `axis` (`toward` -1: as it moves back), `apply` given each size until
 * the press ends.
 */
export function dragSize(
  event: PointerEvent,
  from: number,
  axis: 'x' | 'y',
  apply: (size: number) => void,
  toward: 1 | -1 = 1,
) {
  if (event.button !== 0) return;
  event.preventDefault();
  const handle = event.currentTarget as HTMLElement;
  handle.setPointerCapture(event.pointerId);
  const at = (e: PointerEvent) => (axis === 'x' ? e.clientX : e.clientY);
  const start = at(event);
  const move = (e: PointerEvent) => apply(from + toward * (at(e) - start));
  const end = () => {
    handle.removeEventListener('pointermove', move);
    handle.removeEventListener('pointerup', end);
    handle.removeEventListener('pointercancel', end);
    document.documentElement.classList.remove('resizing-x', 'resizing-y');
  };
  handle.addEventListener('pointermove', move);
  handle.addEventListener('pointerup', end);
  handle.addEventListener('pointercancel', end);
  // the cursor stays the handle's wherever the pointer goes meanwhile
  document.documentElement.classList.add(axis === 'x' ? 'resizing-x' : 'resizing-y');
}

/** A size kept between starts under `key`, else `null`. */
export function savedSize<T>(key: string): T | null {
  try {
    return JSON.parse(localStorage.getItem(key) ?? 'null') as T | null;
  } catch {
    return null;
  }
}

export function saveSize(key: string, size: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(size));
  } catch {
    // storage unavailable: the size lasts until the next start only
  }
}
