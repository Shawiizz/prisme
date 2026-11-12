/** The exact size, in device pixels, of the canvases fitted: the box the browser paints each
 * one in, so that a backing store of that size is drawn pixel for pixel, never resampled
 * (at 125 % or 150 %, CSS pixels times the ratio fall between device pixels). */
const sizes = new WeakMap<Element, { width: number; height: number }>();
const observer = new ResizeObserver((entries) => {
  for (const entry of entries) {
    const box = entry.devicePixelContentBoxSize?.[0];
    if (box) {
      sizes.set(entry.target, {
        width: Math.max(1, box.inlineSize),
        height: Math.max(1, box.blockSize),
      });
    }
  }
});

/** Sets the backing store of `canvas` to its size in device pixels and returns it; the size
 * is estimated until the browser first reports it. */
export function fitToDevicePixels(canvas: HTMLCanvasElement): { width: number; height: number } {
  let size = sizes.get(canvas);
  if (!size) {
    observer.observe(canvas, { box: 'device-pixel-content-box' });
    const ratio = window.devicePixelRatio || 1;
    const rect = canvas.getBoundingClientRect();
    size = {
      width: Math.max(1, Math.round(rect.width * ratio)),
      height: Math.max(1, Math.round(rect.height * ratio)),
    };
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
