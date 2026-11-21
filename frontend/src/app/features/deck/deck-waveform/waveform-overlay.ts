import { Palette } from '../../../shared/palette';
import { hexColor } from '../../../shared/track-colors';
import type { Cue, LoopStatus, Tempo } from '../../../core/models/backend';

/** A window on the track: `fromS` to `fromS + spanS` across `width` device pixels. */
export interface View {
  fromS: number;
  spanS: number;
  width: number;
  height: number;
  /** Device pixels per CSS pixel. */
  ratio: number;
}

/** Share of the half height the loudest slices reach in the scrolling view: the rest holds the
 * beat grid's ticks and markers. */
export const HEADROOM = 0.88;
/** Opacity of a beat's line through the waveform, of its ticks at the edges, and of a bar's
 * line. */
const GRID_BEAT = 0.14;
const GRID_TICK = 0.6;
const GRID_BAR = 0.5;

/** What is drawn over the audio of a waveform, in both views: the beat grid, the loop, the
 * cues, the rolls' edges and the playhead. */
export class OverlayPainter {
  /** The colour of each tag of a cue, written once. */
  private readonly cueColors = new Map<number, string>();

  constructor(private readonly palette: Palette) {}

  /** Before the track and after it, outlined arrows pointing to it: they scroll by as the
   * silence there plays (a record's lead-in). */
  drawRolls(ctx: CanvasRenderingContext2D, durationS: number | null, view: View) {
    const r = view.ratio;
    const width = 6 * r;
    const step = 16 * r;
    const mid = view.height / 2;
    const half = 5 * r;
    const arrow = (tip: number, toTheRight: boolean) => {
      const base = toTheRight ? tip - width : tip + width;
      ctx.moveTo(tip, mid);
      ctx.lineTo(base, mid - half);
      ctx.lineTo(base, mid + half);
      ctx.closePath();
    };
    ctx.beginPath();
    let tip = xOf(0, view) - 4 * r;
    if (tip > view.width) tip -= Math.floor((tip - view.width) / step) * step;
    for (; tip > 0; tip -= step) arrow(tip, true);
    if (durationS !== null) {
      tip = xOf(durationS, view) + 4 * r;
      if (tip < 0) tip += Math.floor(-tip / step) * step;
      for (; tip < view.width; tip += step) arrow(tip, false);
    }
    ctx.strokeStyle = this.palette.text3;
    ctx.lineWidth = r;
    ctx.stroke();
  }

  /** As on a CDJ's screen, discreet so that the music reads first: on each beat a tick at the
   * top and bottom edges, in the room the waveform leaves there, joined by a faint line; on the
   * first beat of each bar a thin line in the bar colour, pointed at from both edges. Only along
   * the track, once its length is known. */
  drawBeatGrid(ctx: CanvasRenderingContext2D, tempo: Tempo, durationS: number | null, view: View) {
    const period = 60 / tempo.bpm;
    const first = Math.max(Math.ceil((view.fromS - tempo.firstBeatS) / period), 0);
    const toS = Math.min(view.fromS + view.spanS, durationS ?? Infinity);
    const last = Math.floor((toS - tempo.firstBeatS) / period);
    if (last < first) return;
    const r = view.ratio;
    const line = Math.max(Math.round(r), 1);
    const left = (n: number) => Math.round(xOf(tempo.firstBeatS + n * period, view) - line / 2);
    const h = view.height;
    const tall = Math.min(5 * r, ((1 - HEADROOM) / 2) * h);
    ctx.fillStyle = this.palette.text;
    const bar = tempo.beatsPerBar;
    for (let n = first; n <= last; n++) {
      if (n % bar === 0) continue;
      const x = left(n);
      ctx.globalAlpha = GRID_BEAT;
      ctx.fillRect(x, tall, line, h - 2 * tall);
      ctx.globalAlpha = GRID_TICK;
      ctx.fillRect(x, 0, line, tall);
      ctx.fillRect(x, h - tall, line, tall);
    }
    ctx.fillStyle = this.palette.bar;
    ctx.globalAlpha = GRID_BAR;
    for (let n = Math.ceil(first / bar) * bar; n <= last; n += bar)
      ctx.fillRect(left(n), 0, line, h);
    ctx.globalAlpha = 1;
    const half = 0.8 * tall;
    ctx.beginPath();
    for (let n = Math.ceil(first / bar) * bar; n <= last; n += bar) {
      const mid = left(n) + line / 2;
      ctx.moveTo(mid - half, 0);
      ctx.lineTo(mid + half, 0);
      ctx.lineTo(mid, tall);
      ctx.closePath();
      ctx.moveTo(mid - half, h);
      ctx.lineTo(mid + half, h);
      ctx.lineTo(mid, h - tall);
      ctx.closePath();
    }
    ctx.fill();
  }

  /** A band of light between two lines; a stored loop as two dim lines. */
  drawLoop(ctx: CanvasRenderingContext2D, loop: LoopStatus | null, view: View) {
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
  drawCues(ctx: CanvasRenderingContext2D, cues: Cue[], view: View, flags: boolean) {
    const r = view.ratio;
    ctx.font = `600 ${10 * r}px ${this.palette.mono}`;
    ctx.textBaseline = 'middle';
    for (const cue of cues) {
      const x = Math.round(xOf(cue.positionS, view));
      if (x < -20 * r || x > view.width) continue;
      ctx.fillStyle = cue.hot === null ? this.palette.light : this.cueColor(cue.color);
      ctx.fillRect(x, 0, r, flags ? view.height : 5 * r);
      if (flags) {
        ctx.fillRect(x, 0, 14 * r, 13 * r);
        ctx.fillStyle = this.palette.onLight;
        ctx.fillText(cue.hot === null ? 'C' : `${cue.hot + 1}`, x + 3.5 * r, 7 * r);
      }
    }
  }

  private cueColor(color: number): string {
    let written = this.cueColors.get(color);
    if (written === undefined) {
      written = hexColor(color);
      this.cueColors.set(color, written);
    }
    return written;
  }

  /** The only white line of the screen, dispersed on its edges like light through a prism. */
  drawPlayhead(ctx: CanvasRenderingContext2D, x: number, view: View) {
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
}

/** Where `seconds` of the track falls across `view` (device pixels). */
export function xOf(seconds: number, view: View): number {
  return ((seconds - view.fromS) / view.spanS) * view.width;
}
