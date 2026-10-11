/** Leaves room around the circle. */
const FILL = 0.88;
/** How fast the zoom follows the signal level (per frame). */
const ZOOM_SMOOTHING = 0.15;
const SMALLEST_SIGNAL = 0.02;

/**
 * Draws a turntable's signal as an XY plot, left channel across, right channel up: a
 * clean timecode traces a round circle; noise, hum or a weak signal show at once. The
 * zoom follows the level so the circle fills the scope whatever the input gain.
 */
export class ScopePainter {
  private radius = 0.2;

  draw(
    ctx: CanvasRenderingContext2D,
    points: [number, number][],
    size: number,
    colors: { dots: string; guides: string },
  ) {
    const centre = size / 2;
    ctx.clearRect(0, 0, size, size);
    this.drawGuides(ctx, centre, colors.guides);

    const loudest = points.reduce((m, [l, r]) => Math.max(m, Math.hypot(l, r)), 0);
    this.radius += (Math.max(loudest, SMALLEST_SIGNAL) - this.radius) * ZOOM_SMOOTHING;
    const scale = (centre * FILL) / this.radius;

    ctx.fillStyle = colors.dots;
    ctx.globalAlpha = 0.55;
    for (const [l, r] of points) {
      ctx.fillRect(centre + l * scale, centre - r * scale, 1.5, 1.5);
    }
    ctx.globalAlpha = 1;
  }

  private drawGuides(ctx: CanvasRenderingContext2D, centre: number, color: string) {
    ctx.strokeStyle = color;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.arc(centre, centre, centre * FILL, 0, Math.PI * 2);
    ctx.moveTo(0, centre);
    ctx.lineTo(centre * 2, centre);
    ctx.moveTo(centre, 0);
    ctx.lineTo(centre, centre * 2);
    ctx.stroke();
  }
}
