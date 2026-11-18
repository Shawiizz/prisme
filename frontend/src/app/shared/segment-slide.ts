/** How long the light takes to slide from one choice to the next. */
const SLIDE_MS = 240;
const EASE = 'cubic-bezier(0.25, 0.8, 0.25, 1)';

/**
 * In every row of choices of the page (`.pr-seg`), the light slides from the choice it was on to
 * the new one, rather than jumping: whenever the choice changes, whatever changed it (a press, a
 * controller, the engine answering), a light of its own goes from one to the other while the
 * choices show none, then is gone. What the row holds does not change.
 */
export function slideSegments() {
  if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;
  // seen before the page is drawn again: the choice left is still where it was
  new MutationObserver((records) => {
    const moves = new Map<HTMLElement, { from?: HTMLElement; to?: HTMLElement }>();
    for (const record of records) {
      const button = record.target as HTMLElement;
      const row = button.parentElement;
      if (button.tagName !== 'BUTTON' || !row?.classList.contains('pr-seg')) continue;
      const move = moves.get(row) ?? {};
      moves.set(row, move);
      const chosen = button.getAttribute('aria-checked') === 'true';
      const was = record.oldValue === 'true';
      if (was && !chosen) move.from = button;
      if (!was && chosen) move.to = button;
    }
    for (const [row, { from, to }] of moves) {
      if (from && to) slide(row, from, to);
    }
  }).observe(document.body, {
    subtree: true,
    attributes: true,
    attributeFilter: ['aria-checked'],
    attributeOldValue: true,
  });
}

interface Place {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Where `element` lies in `row`, inside its border. */
function place(element: Element, row: HTMLElement): Place {
  const e = element.getBoundingClientRect();
  const r = row.getBoundingClientRect();
  return {
    x: e.left - r.left - row.clientLeft,
    y: e.top - r.top - row.clientTop,
    width: e.width,
    height: e.height,
  };
}

/** Slides the row's light from where it is (or from `from`) to `to`. */
function slide(row: HTMLElement, from: HTMLElement, to: HTMLElement) {
  const sliding = row.querySelector<HTMLElement>(':scope > .pr-seg__light');
  // pressed again while it slides: it goes on from where it is
  const start = place(sliding ?? from, row);
  const end = place(to, row);
  sliding?.remove();
  const light = document.createElement('span');
  light.className = 'pr-seg__light';
  const put = ({ x, y, width, height }: Place) =>
    Object.assign(light.style, {
      transform: `translate(${x}px, ${y}px)`,
      width: `${width}px`,
      height: `${height}px`,
    });
  put(start);
  row.prepend(light);
  row.classList.add('is-sliding');
  light.getBoundingClientRect();
  light.style.transition = ['transform', 'width', 'height']
    .map((property) => `${property} ${SLIDE_MS}ms ${EASE}`)
    .join(', ');
  put(end);
  setTimeout(() => {
    if (!light.isConnected) return;
    light.remove();
    row.classList.remove('is-sliding');
  }, SLIDE_MS + 20);
}
