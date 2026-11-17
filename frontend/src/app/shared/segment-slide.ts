/** How long the light takes to slide from one choice to the next. */
const SLIDE_MS = 200;
const EASE = 'cubic-bezier(0.2, 0.8, 0.2, 1)';
/** The choice in the light, in a row of choices (`.pr-seg`). */
const CHOSEN = ':scope > button[aria-checked="true"], :scope > button.is-on';

/**
 * In every row of choices of the page, the light slides from the choice it was on to the one
 * pressed, rather than jumping: a light of its own goes from one to the other while the
 * choices themselves show none, then is gone. What the row holds does not change.
 */
export function slideSegments() {
  if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;
  document.addEventListener(
    'click',
    (event) => {
      const pressed = (event.target as Element).closest?.('.pr-seg > button');
      const row = pressed?.parentElement;
      if (!row || row.querySelectorAll(CHOSEN).length !== 1) return;
      const from = row.querySelector<HTMLElement>(CHOSEN);
      if (!from || from === pressed) return;
      const before = place(from, row);
      // the choice is made once the click has run: the light goes where it went
      requestAnimationFrame(() => {
        const to = row.querySelector<HTMLElement>(CHOSEN);
        if (to && to !== from) slide(row, before, place(to, row));
      });
    },
    true,
  );
}

interface Place {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Where `button` lies in `row`, inside its border. */
function place(button: HTMLElement, row: HTMLElement): Place {
  const b = button.getBoundingClientRect();
  const r = row.getBoundingClientRect();
  return {
    left: b.left - r.left - row.clientLeft,
    top: b.top - r.top - row.clientTop,
    width: b.width,
    height: b.height,
  };
}

function slide(row: HTMLElement, from: Place, to: Place) {
  row.querySelector(':scope > .pr-seg__light')?.remove();
  const light = document.createElement('span');
  light.className = 'pr-seg__light';
  const put = ({ left, top, width, height }: Place) =>
    Object.assign(light.style, {
      left: `${left}px`,
      top: `${top}px`,
      width: `${width}px`,
      height: `${height}px`,
    });
  put(from);
  row.prepend(light);
  row.classList.add('is-sliding');
  light.getBoundingClientRect();
  light.style.transition = ['left', 'top', 'width', 'height']
    .map((property) => `${property} ${SLIDE_MS}ms ${EASE}`)
    .join(', ');
  put(to);
  setTimeout(() => {
    light.remove();
    if (!row.querySelector(':scope > .pr-seg__light')) row.classList.remove('is-sliding');
  }, SLIDE_MS);
}
