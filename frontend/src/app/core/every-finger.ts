/** What a finger presses: a key, or what stands for one. */
const KEYS = 'button, [role="radio"], [role="switch"], label';
/** A click the browser would make after one of ours is the same press (ms). */
const SAME_PRESS_MS = 400;

/**
 * A browser makes clicks of the first finger on the screen only: a key pressed with
 * another finger meanwhile (both PLAY at once) would not answer. Here every other finger
 * lifted from the key it pressed clicks that key too, as one pointer of a mouse would.
 */
export function clickWithEveryFinger() {
  const pressed = new Map<number, Element>();
  const clicked = new WeakMap<Element, number>();

  addEventListener(
    'pointerdown',
    (event) => {
      if (event.pointerType !== 'touch' || event.isPrimary) return;
      const key = (event.target as Element).closest(KEYS);
      if (key) pressed.set(event.pointerId, key);
    },
    true,
  );
  addEventListener(
    'pointerup',
    (event) => {
      const key = pressed.get(event.pointerId);
      pressed.delete(event.pointerId);
      const under = document.elementFromPoint(event.clientX, event.clientY);
      if (!key || !under || !key.contains(under)) return;
      clicked.set(key, event.timeStamp);
      key.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, detail: 1 }));
    },
    true,
  );
  addEventListener('pointercancel', (event) => pressed.delete(event.pointerId), true);
  // should the browser click it as well, the press has already been answered
  addEventListener(
    'click',
    (event) => {
      if (!event.isTrusted) return;
      const key = (event.target as Element | null)?.closest?.(KEYS);
      const at = key && clicked.get(key);
      if (at !== undefined && at !== null && event.timeStamp - at < SAME_PRESS_MS) {
        event.preventDefault();
        event.stopImmediatePropagation();
      }
    },
    true,
  );
}
