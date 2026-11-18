/**
 * A popover opened at a point: below it, or above it in the lower half of the window; to
 * its right, or to its left in the right half.
 */
export interface Popover {
  left: number | null;
  right: number | null;
  top: number | null;
  bottom: number | null;
}

export function popoverAt(x: number, y: number): Popover {
  const below = y < window.innerHeight / 2;
  const rightwards = x < window.innerWidth / 2;
  return {
    left: rightwards ? x : null,
    right: rightwards ? null : window.innerWidth - x,
    top: below ? y : null,
    bottom: below ? null : window.innerHeight - y,
  };
}

export function popoverAtPointer(event: MouseEvent): Popover {
  return popoverAt(event.clientX, event.clientY);
}

/** A popover above an element (the buttons of the selection bar). */
export function popoverAbove(element: HTMLElement): Popover {
  const r = element.getBoundingClientRect();
  return { ...popoverAt(r.left, r.top), top: null, bottom: window.innerHeight - r.top + 4 };
}

/** A popover beside an element, on its right: below its top, or above its bottom in the lower
 * half of the window (the button at the end of a line). */
export function popoverBeside(element: HTMLElement): Popover {
  const r = element.getBoundingClientRect();
  const below = r.top < window.innerHeight / 2;
  return {
    left: r.right + 4,
    right: null,
    top: below ? r.top : null,
    bottom: below ? null : window.innerHeight - r.bottom,
  };
}
