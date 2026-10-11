import { Cue, LoopStatus, Tempo, Waveform } from '../../../core/models/engine';

/** What is drawn over the waveform. */
export interface WaveformOverlay {
  tempo: Tempo | null;
  cues: Cue[];
  loop: LoopStatus | null;
}

/** A window on the track: `fromS` to `fromS + spanS` across `width` pixels. */
interface View {
  fromS: number;
  spanS: number;
  width: number;
  height: number;
}

const MAIN_CUE_COLOR = '#ffb02e';
const LOOP_ACTIVE = { fill: 'rgba(52, 230, 160, 0.16)', edge: '#34e6a0' };
const LOOP_STORED = { fill: 'rgba(255, 255, 255, 0.05)', edge: 'rgba(255, 255, 255, 0.35)' };

/**
 * Draws waveforms on a canvas. Height = peak amplitude, colour = balance between the
 * low (red), mid (green) and high (blue) bands, like most DJ software.
 */
export class WaveformPainter {
  /** Colour of slice `i` as an rgb() string, cached per waveform. */
  private colors: string[] = [];
  private source: Waveform | null = null;

  /**
   * Scrolling view centred on `positionS`: past audio dimmed, beat grid (stronger every
   * 4 beats), loop, cues and the playhead. Without a waveform yet (track being loaded),
   * everything but the audio.
   */
  drawZoomed(
    ctx: CanvasRenderingContext2D,
    w: Waveform | null,
    overlay: WaveformOverlay,
    positionS: number,
    spanS: number,
    size: { width: number; height: number },
  ) {
    const view: View = { fromS: positionS - spanS / 2, spanS, ...size };
    ctx.clearRect(0, 0, view.width, view.height);
    this.drawLoop(ctx, overlay.loop, view);
    if (w) this.drawZoomedAudio(ctx, w, view);
    if (overlay.tempo) this.drawBeatGrid(ctx, overlay.tempo, view);
    this.drawCues(ctx, overlay.cues, view, true);
    ctx.fillStyle = '#ffffff';
    ctx.fillRect(Math.floor(view.width / 2), 0, 2, view.height);
  }

  private drawZoomedAudio(ctx: CanvasRenderingContext2D, w: Waveform, view: View) {
    const sps = w.slicesPerSecond;
    const slicesPerPx = (view.spanS * sps) / view.width;
    const mid = view.height / 2;
    for (let x = 0; x < view.width; x++) {
      const from = Math.floor(view.fromS * sps + x * slicesPerPx);
      const to = Math.max(from + 1, Math.floor(view.fromS * sps + (x + 1) * slicesPerPx));
      if (to <= 0 || from >= w.peak.length) continue;
      let best = Math.max(from, 0);
      for (let i = best; i < Math.min(to, w.peak.length); i++) {
        if (w.peak[i] > w.peak[best]) best = i;
      }
      const amp = (w.peak[best] / 255) * (mid - 2);
      ctx.globalAlpha = x < view.width / 2 ? 0.45 : 1;
      ctx.fillStyle = this.colorAt(w, best);
      ctx.fillRect(x, mid - amp, 1, amp * 2);
    }
    ctx.globalAlpha = 1;
  }

  /** Whole track, played part dimmed, with loop, cues and the playhead. */
  drawOverview(
    ctx: CanvasRenderingContext2D,
    w: Waveform | null,
    overlay: WaveformOverlay,
    positionS: number,
    durationS: number,
    size: { width: number; height: number },
  ) {
    const view: View = { fromS: 0, spanS: durationS, ...size };
    ctx.clearRect(0, 0, view.width, view.height);
    this.drawLoop(ctx, overlay.loop, view);
    const playedX = durationS > 0 ? (positionS / durationS) * view.width : 0;
    if (w) this.drawOverviewAudio(ctx, w, view, playedX);
    this.drawCues(ctx, overlay.cues, view, false);
    ctx.fillStyle = '#ffffff';
    ctx.fillRect(Math.floor(playedX), 0, 1, view.height);
  }

  private drawOverviewAudio(
    ctx: CanvasRenderingContext2D,
    w: Waveform,
    view: View,
    playedX: number,
  ) {
    const n = w.peak.length;
    const mid = view.height / 2;
    for (let x = 0; x < view.width; x++) {
      const from = Math.floor((x / view.width) * n);
      const to = Math.max(from + 1, Math.floor(((x + 1) / view.width) * n));
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
  }

  private drawBeatGrid(ctx: CanvasRenderingContext2D, tempo: Tempo, view: View) {
    const period = 60 / tempo.bpm;
    const first = Math.ceil((view.fromS - tempo.firstBeatS) / period);
    const last = Math.floor((view.fromS + view.spanS - tempo.firstBeatS) / period);
    for (let n = Math.max(first, 0); n <= last; n++) {
      const x = Math.round(xOf(tempo.firstBeatS + n * period, view));
      const bar = n % 4 === 0;
      ctx.fillStyle = bar ? 'rgba(255,255,255,0.55)' : 'rgba(255,255,255,0.2)';
      ctx.fillRect(x, 0, bar ? 2 : 1, view.height);
    }
  }

  private drawLoop(ctx: CanvasRenderingContext2D, loop: LoopStatus | null, view: View) {
    if (loop?.loopInS != null) {
      ctx.fillStyle = LOOP_ACTIVE.edge;
      ctx.fillRect(Math.round(xOf(loop.loopInS, view)), 0, 2, view.height);
    }
    if (loop?.startS == null || loop.endS == null) return;
    const style = loop.active ? LOOP_ACTIVE : LOOP_STORED;
    const x0 = xOf(loop.startS, view);
    const x1 = xOf(loop.endS, view);
    ctx.fillStyle = style.fill;
    ctx.fillRect(x0, 0, x1 - x0, view.height);
    ctx.fillStyle = style.edge;
    ctx.fillRect(Math.round(x0), 0, 2, view.height);
    ctx.fillRect(Math.round(x1) - 2, 0, 2, view.height);
  }

  /** A line per cue; with `flags`, a numbered tab at the top. */
  private drawCues(ctx: CanvasRenderingContext2D, cues: Cue[], view: View, flags: boolean) {
    ctx.font = '700 10px Inter, system-ui, sans-serif';
    ctx.textBaseline = 'middle';
    for (const cue of cues) {
      const x = Math.round(xOf(cue.positionS, view));
      if (x < -20 || x > view.width) continue;
      const color =
        cue.hot === null ? MAIN_CUE_COLOR : `#${cue.color.toString(16).padStart(6, '0')}`;
      ctx.fillStyle = color;
      ctx.fillRect(x, 0, flags ? 2 : 1, view.height);
      if (flags) {
        ctx.fillRect(x, 0, 14, 13);
        ctx.fillStyle = '#07080d';
        ctx.fillText(cue.hot === null ? 'C' : `${cue.hot + 1}`, x + 4, 7);
      }
    }
  }

  private colorAt(w: Waveform, i: number): string {
    if (this.source !== w) {
      this.source = w;
      this.colors = Array.from(w.peak, (_, k) => {
        const l = w.low[k],
          m = w.mid[k],
          h = w.high[k];
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
}

function xOf(seconds: number, view: View): number {
  return ((seconds - view.fromS) / view.spanS) * view.width;
}
