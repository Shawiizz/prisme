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

/** Height of each band, as a share of the lane: low behind, high in front. */
const LAYERS = [
  { band: 'low', height: 0.94 },
  { band: 'mid', height: 0.66 },
  { band: 'high', height: 0.4 },
] as const;
/** Opacity of what has played. */
const PLAYED = 0.42;

/** The colours of the design system (tokens.css), read once. */
class Palette {
  private readonly style = getComputedStyle(document.documentElement);
  readonly low = this.token('--pr-f-low');
  readonly mid = this.token('--pr-f-mid');
  readonly high = this.token('--pr-f-high');
  readonly light = this.token('--pr-light');
  readonly veil = this.token('--pr-light-veil');
  readonly onLight = this.token('--pr-on-light');
  readonly text = this.token('--pr-text');
  readonly text2 = this.token('--pr-text-2');
  readonly text3 = this.token('--pr-text-3');
  readonly mono = this.token('--pr-font-mono');

  private token(name: string): string {
    return this.style.getPropertyValue(name).trim();
  }
}

/**
 * Draws the waveforms (design/DESIGN.md, Waveform): the three bands as layers in the
 * spectrum's colours, the beat grid as short ticks, cues as flags in their tag colour, the
 * loop as a band of light and the playhead, the only white line, dispersed on its edges.
 */
export class WaveformPainter {
  private readonly palette = new Palette();
  /** Highest value of each band over the track, so that every band fills its layer. */
  private bandMax = { low: 1, mid: 1, high: 1 };
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
    if (w) this.drawLayers(ctx, w, view);
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

  private drawLayers(ctx: CanvasRenderingContext2D, w: Waveform, view: View) {
    this.measure(w);
    const sps = w.slicesPerSecond;
    const slicesPerPx = (view.spanS * sps) / view.width;
    const mid = view.height / 2;
    const playhead = view.width / 2;
    for (const layer of LAYERS) {
      const values = w[layer.band];
      const scale = (layer.height * mid) / this.bandMax[layer.band];
      ctx.fillStyle = this.palette[layer.band];
      for (const played of [true, false]) {
        ctx.globalAlpha = played ? PLAYED : 1;
        ctx.beginPath();
        const [from, to] = played ? [0, Math.floor(playhead)] : [Math.floor(playhead), view.width];
        for (let x = from; x < to; x++) {
          const first = Math.floor(view.fromS * sps + x * slicesPerPx);
          const last = Math.max(first + 1, Math.floor(view.fromS * sps + (x + 1) * slicesPerPx));
          if (last <= 0 || first >= values.length) continue;
          let value = 0;
          for (let i = Math.max(first, 0); i < Math.min(last, values.length); i++) {
            value = Math.max(value, values[i]);
          }
          const h = value * scale;
          ctx.rect(x, mid - h, 1, h * 2);
        }
        ctx.fill();
      }
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
    for (const played of [true, false]) {
      ctx.fillStyle = played ? this.palette.text3 : this.palette.text2;
      ctx.globalAlpha = played ? 0.55 : 0.9;
      ctx.beginPath();
      const [from, to] = played ? [0, Math.floor(playedX)] : [Math.floor(playedX), view.width];
      for (let x = from; x < to; x++) {
        const first = Math.floor((x / view.width) * n);
        const last = Math.max(first + 1, Math.floor(((x + 1) / view.width) * n));
        let peak = 0;
        for (let i = first; i < Math.min(last, n); i++) peak = Math.max(peak, w.peak[i]);
        const h = (peak / 255) * (mid - 1);
        ctx.rect(x, mid - h, 1, h * 2);
      }
      ctx.fill();
    }
    ctx.globalAlpha = 1;
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

  private measure(w: Waveform) {
    if (this.source === w) return;
    this.source = w;
    const max = (values: Uint8Array) => values.reduce((m, v) => Math.max(m, v), 1);
    this.bandMax = { low: max(w.low), mid: max(w.mid), high: max(w.high) };
  }
}

function xOf(seconds: number, view: View): number {
  return ((seconds - view.fromS) / view.spanS) * view.width;
}
