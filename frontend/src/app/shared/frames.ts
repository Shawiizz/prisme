import { DestroyRef, afterNextRender, inject } from '@angular/core';

/**
 * Calls `draw` on every animation frame, outside change detection, from the first render
 * until the component goes: at the first render itself too, so that the first frame painted
 * already shows the drawing. To be called where `inject` works.
 */
export function everyFrame(draw: () => void) {
  let frame = 0;
  const loop = () => {
    draw();
    frame = requestAnimationFrame(loop);
  };
  afterNextRender(loop);
  inject(DestroyRef).onDestroy(() => cancelAnimationFrame(frame));
}

/**
 * What a frame shows, value after value, against what the last frame showed: a frame
 * showing the same is not drawn again (a deck at rest costs nothing). Kept in one array,
 * nothing made per frame.
 */
export class Showing {
  private readonly values: unknown[] = [];
  private count = 0;
  private changed = false;

  /** A frame starts. */
  start(): this {
    this.count = 0;
    this.changed = false;
    return this;
  }

  /** One more value the frame shows. */
  with(value: unknown): this {
    if (this.count >= this.values.length || this.values[this.count] !== value) {
      this.values[this.count] = value;
      this.changed = true;
    }
    this.count++;
    return this;
  }

  /** Whether the frame shows something the last did not. */
  isNew(): boolean {
    if (this.values.length !== this.count) {
      this.values.length = this.count;
      this.changed = true;
    }
    return this.changed;
  }

  /** The next frame is drawn whatever it shows. */
  forget() {
    this.values.length = 0;
  }
}
