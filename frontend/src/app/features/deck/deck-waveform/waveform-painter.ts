import { Cue, LoopStatus, Tempo, Waveform } from '../../../core/models/engine';

/** What is drawn over the waveform. */
export interface WaveformOverlay {
  tempo: Tempo | null;
  cues: Cue[];
  loop: LoopStatus | null;
}

/** A window on the track: `fromS` to `fromS + spanS` across `width` device pixels. */
interface View {
  fromS: number;
  spanS: number;
  width: number;
  height: number;
  /** Device pixels per CSS pixel. */
  ratio: number;
}

/** Opacity of what has played, in the scrolling view and in the overview. */
const PLAYED = 0.45;
const PLAYED_OVERVIEW = 0.35;

/** The colours of the design system (tokens.css), read once. */
class Palette {
  private readonly style = getComputedStyle(document.documentElement);
  readonly low = this.token('--pr-f-low');
  readonly high = this.token('--pr-f-high');
  readonly light = this.token('--pr-light');
  readonly veil = this.token('--pr-light-veil');
  readonly onLight = this.token('--pr-on-light');
  readonly text = this.token('--pr-text');
  readonly text3 = this.token('--pr-text-3');
  readonly mono = this.token('--pr-font-mono');

  private token(name: string): string {
    return this.style.getPropertyValue(name).trim();
  }
}

/**
 * Draws the waveforms (design/DESIGN.md, Waveform): one envelope coloured slice by slice by
 * the balance of its bands, the beat grid as short ticks, cues as flags in their tag colour,
 * the loop as a band of light and the playhead, the only white line, dispersed on its edges.
 */
export class WaveformPainter {
  private readonly palette = new Palette();
  /** Colour of each slice, cached per waveform. */
  private colors: string[] = [];
  private source: Waveform | null = null;

  /** Scrolling view centred on `positionS`; without a waveform yet, everything but the audio. */
  drawZoomed(
    ctx: CanvasRenderingContext2D,
    w: Waveform | null,
    overlay: WaveformOverlay,
    positionS: number,
    spanS: number,
    size: { width: number; height: number; ratio: number },
  ) {
    const view: View = { fromS: positionS - spanS / 2, spanS, ...size };
    ctx.clearRect(0, 0, view.width, view.height);
    this.drawLoop(ctx, overlay.loop, view);
    if (w) this.drawZoomedAudio(ctx, w, view);
    if (overlay.tempo) this.drawBeatGrid(ctx, overlay.tempo, view);
    this.drawCues(ctx, overlay.cues, view, true);
    this.drawPlayhead(ctx, Math.round(view.width / 2), view);
  }

  /** Whole track in grey, played part dimmed, with the loop, cue ticks and the position. */
  drawOverview(
    ctx: CanvasRenderingContext2D,
    w: Waveform | null,
    overlay: WaveformOverlay,
    positionS: number,
    durationS: number,
    size: { width: number; height: number; ratio: number },
  ) {
    const view: View = { fromS: 0, spanS: durationS, ...size };
    ctx.clearRect(0, 0, view.width, view.height);
    this.drawLoop(ctx, overlay.loop, view);
    const playedX = durationS > 0 ? (positionS / durationS) * view.width : 0;
    if (w) this.drawOverviewAudio(ctx, w, view, playedX);
    this.drawCues(ctx, overlay.cues, view, false);
    ctx.fillStyle = this.palette.light;
    ctx.fillRect(Math.floor(playedX) - view.ratio, 0, 2 * view.ratio, view.height);
  }

  private drawZoomedAudio(ctx: CanvasRenderingContext2D, w: Waveform, view: View) {
    const sps = w.slicesPerSecond;
    const slicesPerPx = (view.spanS * sps) / view.width;
    const mid = view.height / 2;
    const playhead = view.width / 2;
    for (let x = 0; x < view.width; x++) {
      const first = Math.floor(view.fromS * sps + x * slicesPerPx);
      const last = Math.max(first + 1, Math.floor(view.fromS * sps + (x + 1) * slicesPerPx));
      if (last <= 0 || first >= w.peak.length) continue;
      const loudest = this.loudest(w, first, last);
      const h = (w.peak[loudest] / 255) * (mid - 1);
      ctx.globalAlpha = x < playhead ? PLAYED : 1;
      ctx.fillStyle = this.colorAt(w, loudest);
      ctx.fillRect(x, mid - h, 1, h * 2);
    }
    ctx.globalAlpha = 1;
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
      const first = Math.floor((x / view.width) * n);
      const last = Math.max(first + 1, Math.floor(((x + 1) / view.width) * n));
      const loudest = this.loudest(w, first, last);
      const h = (w.peak[loudest] / 255) * (mid - 1);
      ctx.globalAlpha = x < playedX ? PLAYED_OVERVIEW : 1;
      ctx.fillStyle = this.colorAt(w, loudest);
      ctx.fillRect(x, mid - h, 1, h * 2);
    }
    ctx.globalAlpha = 1;
  }

  /** The loudest slice from `first` to `last` (excluded), within the track. */
  private loudest(w: Waveform, first: number, last: number): number {
    let best = Math.max(first, 0);
    for (let i = best; i < Math.min(last, w.peak.length); i++) {
      if (w.peak[i] > w.peak[best]) best = i;
    }
    return best;
  }

  /** Short ticks at the top and the bottom, longer and brighter on each bar. */
  private drawBeatGrid(ctx: CanvasRenderingContext2D, tempo: Tempo, view: View) {
    const period = 60 / tempo.bpm;
    const first = Math.ceil((view.fromS - tempo.firstBeatS) / period);
    const last = Math.floor((view.fromS + view.spanS - tempo.firstBeatS) / period);
    ctx.fillStyle = this.palette.text;
    for (let n = Math.max(first, 0); n <= last; n++) {
      const x = Math.round(xOf(tempo.firstBeatS + n * period, view));
      const bar = n % 4 === 0;
      const length = (bar ? 9 : 5) * view.ratio;
      ctx.globalAlpha = bar ? 0.75 : 0.35;
      ctx.fillRect(x, 0, view.ratio, length);
      ctx.fillRect(x, view.height - length, view.ratio, length);
    }
    ctx.globalAlpha = 1;
  }

  /** A band of light between two lines; a stored loop as two dim lines. */
  private drawLoop(ctx: CanvasRenderingContext2D, loop: LoopStatus | null, view: View) {
    const line = view.ratio;
    if (loop?.loopInS != null) {
      ctx.fillStyle = this.palette.light;
      ctx.fillRect(Math.round(xOf(loop.loopInS, view)), 0, line, view.height);
    }
    if (loop?.startS == null || loop.endS == null) return;
    const x0 = Math.round(xOf(loop.startS, view));
    const x1 = Math.round(xOf(loop.endS, view));
    if (loop.active) {
      ctx.fillStyle = this.palette.veil;
      ctx.fillRect(x0, 0, x1 - x0, view.height);
    }
    ctx.fillStyle = loop.active ? this.palette.light : this.palette.text3;
    ctx.fillRect(x0, 0, line, view.height);
    ctx.fillRect(x1 - line, 0, line, view.height);
  }

  /** A line per cue in its tag colour; with `flags`, a numbered flag at the top. */
  private drawCues(ctx: CanvasRenderingContext2D, cues: Cue[], view: View, flags: boolean) {
    const r = view.ratio;
    ctx.font = `600 ${10 * r}px ${this.palette.mono}`;
    ctx.textBaseline = 'middle';
    for (const cue of cues) {
      const x = Math.round(xOf(cue.positionS, view));
      if (x < -20 * r || x > view.width) continue;
      const color =
        cue.hot === null ? this.palette.light : `#${cue.color.toString(16).padStart(6, '0')}`;
      ctx.fillStyle = color;
      ctx.fillRect(x, 0, r, flags ? view.height : 5 * r);
      if (flags) {
        ctx.fillRect(x, 0, 14 * r, 13 * r);
        ctx.fillStyle = this.palette.onLight;
        ctx.fillText(cue.hot === null ? 'C' : `${cue.hot + 1}`, x + 3.5 * r, 7 * r);
      }
    }
  }

  /** The only white line of the screen, dispersed on its edges like light through a prism. */
  private drawPlayhead(ctx: CanvasRenderingContext2D, x: number, view: View) {
    const r = view.ratio;
    ctx.globalAlpha = 0.75;
    ctx.fillStyle = this.palette.low;
    ctx.fillRect(x - 2 * r, 0, r, view.height);
    ctx.globalAlpha = 0.85;
    ctx.fillStyle = this.palette.high;
    ctx.fillRect(x + r, 0, r, view.height);
    ctx.globalAlpha = 1;
    ctx.fillStyle = this.palette.light;
    ctx.fillRect(x - r, 0, 2 * r, view.height);
  }

  /**
   * The bands mixed like light: the low band lights the red, the mid the green, the high
   * the blue, scaled to the strongest so that the colours stay vivid whatever the level.
   */
  private colorAt(w: Waveform, i: number): string {
    if (this.source !== w) {
      this.source = w;
      this.colors = Array.from(w.peak, (_, k) => {
        const max = Math.max(w.low[k], w.mid[k], w.high[k], 1);
        const r = Math.round((w.low[k] / max) * 255);
        const g = Math.round((w.mid[k] / max) * 230);
        const b = Math.round((w.high[k] / max) * 255);
        return `rgb(${r},${g},${b})`;
      });
    }
    return this.colors[i];
  }
}

function xOf(seconds: number, view: View): number {
  return ((seconds - view.fromS) / view.spanS) * view.width;
}
