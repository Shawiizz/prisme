import { Waveform } from '../../../core/models/engine';

/**
 * Draws waveforms on a canvas. Height = peak amplitude, colour = balance between the
 * low (red), mid (green) and high (blue) bands, like most DJ software.
 */
export class WaveformPainter {
  /** Colour of slice `i` as an rgb() string, cached per waveform. */
  private colors: string[] = [];
  private source: Waveform | null = null;

  private colorAt(w: Waveform, i: number): string {
    if (this.source !== w) {
      this.source = w;
      this.colors = w.peak.map((_, k) => {
        const l = w.low[k], m = w.mid[k], h = w.high[k];
        const max = Math.max(l, m, h, 1);
        // keep colours vivid whatever the loudness
        const r = Math.round((l / max) * 255);
        const g = Math.round((m / max) * 230);
        const b = Math.round((h / max) * 255);
        return `rgb(${r},${g},${b})`;
      });
    }
    return this.colors[i];
  }

  /**
   * Scrolling view centred on `positionS`, showing `spanS` seconds.
   * Past audio is dimmed; a white line marks the playhead.
   */
  drawZoomed(ctx: CanvasRenderingContext2D, w: Waveform, positionS: number, spanS: number, width: number, height: number) {
    ctx.clearRect(0, 0, width, height);
    const sps = w.slicesPerSecond;
    const startS = positionS - spanS / 2;
    const slicesPerPx = (spanS * sps) / width;
    const mid = height / 2;
    for (let x = 0; x < width; x++) {
      const from = Math.floor((startS * sps) + x * slicesPerPx);
      const to = Math.max(from + 1, Math.floor(startS * sps + (x + 1) * slicesPerPx));
      if (to <= 0 || from >= w.peak.length) continue;
      let best = Math.max(from, 0);
      for (let i = best; i < Math.min(to, w.peak.length); i++) {
        if (w.peak[i] > w.peak[best]) best = i;
      }
      const amp = (w.peak[best] / 255) * (mid - 2);
      ctx.globalAlpha = x < width / 2 ? 0.45 : 1;
      ctx.fillStyle = this.colorAt(w, best);
      ctx.fillRect(x, mid - amp, 1, amp * 2);
    }
    ctx.globalAlpha = 1;
    ctx.fillStyle = '#ffffff';
    ctx.fillRect(Math.floor(width / 2), 0, 2, height);
  }

  /** Whole track, played part dimmed, with the playhead. */
  drawOverview(ctx: CanvasRenderingContext2D, w: Waveform, positionS: number, durationS: number, width: number, height: number) {
    ctx.clearRect(0, 0, width, height);
    const n = w.peak.length;
    const mid = height / 2;
    const playedX = durationS > 0 ? (positionS / durationS) * width : 0;
    for (let x = 0; x < width; x++) {
      const from = Math.floor((x / width) * n);
      const to = Math.max(from + 1, Math.floor(((x + 1) / width) * n));
      let best = from;
      for (let i = from; i < Math.min(to, n); i++) {
        if (w.peak[i] > w.peak[best]) best = i;
      }
      const amp = (w.peak[best] / 255) * (mid - 1);
      ctx.globalAlpha = x < playedX ? 0.3 : 0.9;
      ctx.fillStyle = this.colorAt(w, best);
      ctx.fillRect(x, mid - amp, 1, amp * 2);
    }
    ctx.globalAlpha = 1;
    ctx.fillStyle = '#ffffff';
    ctx.fillRect(Math.floor(playedX), 0, 1, height);
  }
}
