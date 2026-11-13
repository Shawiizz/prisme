/** The exact size, in device pixels, of the canvases fitted: the box the browser paints each
 * one in, so that a backing store of that size is drawn pixel for pixel, never resampled
 * (at 125 % or 150 %, CSS pixels times the ratio fall between device pixels). */
const sizes = new WeakMap<Element, { width: number; height: number }>();
const observer = new ResizeObserver((entries) => {
  for (const entry of entries) {
    const box = entry.devicePixelContentBoxSize?.[0];
    sizes.set(
      entry.target,
      box ? { width: box.inlineSize, height: box.blockSize } : estimated(entry.target),
    );
  }
});

/** The size of `element` in device pixels as its CSS size gives it: what a browser that
 * does not tell the exact one (Safari's engine) is asked. */
function estimated(element: Element): { width: number; height: number } {
  const ratio = window.devicePixelRatio || 1;
  const rect = element.getBoundingClientRect();
  return {
    width: Math.max(1, Math.round(rect.width * ratio)),
    height: Math.max(1, Math.round(rect.height * ratio)),
  };
}

/** Sets the backing store of `canvas` to its size in device pixels and returns it; the size
 * is estimated until the browser first reports it. */
export function fitToDevicePixels(canvas: HTMLCanvasElement): { width: number; height: number } {
  let size = sizes.get(canvas);
  if (!size) {
    try {
      observer.observe(canvas, { box: 'device-pixel-content-box' });
    } catch {
      // a browser without that box: its changes of size are followed all the same
      observer.observe(canvas);
    }
    size = estimated(canvas);
    sizes.set(canvas, size);
  }
  if (canvas.width !== size.width || canvas.height !== size.height) {
    canvas.width = size.width;
    canvas.height = size.height;
  }
  return size;
}

/** Stops following `canvas`, gone from the page. */
export function forgetCanvas(canvas: HTMLCanvasElement) {
  observer.unobserve(canvas);
  sizes.delete(canvas);
}
