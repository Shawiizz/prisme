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
/** Stem levels (0 to 255, -48 to 0 dBFS) below this are another stem's bleed: not drawn. */
const LANE_FLOOR = 80;
/** Width of the tiles the scrolling view's audio is drawn in once (device pixels). */
const TILE_PX = 1024;

/** The colours of the design system (tokens.css), read once. */
class Palette {
  private readonly style = getComputedStyle(document.documentElement);
  readonly low = this.token('--pr-f-low');
  readonly high = this.token('--pr-f-high');
  readonly light = this.token('--pr-light');
  readonly base = this.token('--pr-base');
  readonly void = this.token('--pr-void');
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
  /** The overview's audio as it plays and as played, drawn once per waveform and size. */
  private overview: {
    w: Waveform;
    width: number;
    height: number;
    full: OffscreenCanvas;
    played: OffscreenCanvas;
  } | null = null;

  /** The scrolling view's audio in tiles, at its scale with the track at its own tempo:
   * drawn once each, then copied (stretched to the tempo) frame after frame. */
  private tiles: {
    w: Waveform;
    pxPerS: number;
    height: number;
    images: Map<number, OffscreenCanvas>;
  } | null = null;

  /** The stem lanes, drawn again only for other levels or another size. */
  private lanes: {
    activity: Uint8Array[];
    width: number;
    height: number;
    image: OffscreenCanvas;
  } | null = null;

  /** A lane per part of a separated track, the vocals at the top, lit where the part plays;
   * the played part veiled as in the overview. */
  drawStemLanes(
    ctx: CanvasRenderingContext2D,
    activity: Uint8Array[],
    positionS: number,
    durationS: number,
    size: { width: number; height: number; ratio: number },
  ) {
    ctx.clearRect(0, 0, size.width, size.height);
    ctx.drawImage(this.lanesImage(activity, size), 0, 0);
    const played = durationS > 0 ? Math.round((positionS / durationS) * size.width) : 0;
    ctx.globalAlpha = 1 - PLAYED_OVERVIEW;
    ctx.fillStyle = this.palette.base;
    ctx.fillRect(0, 0, Math.min(Math.max(played, 0), size.width), size.height);
    ctx.globalAlpha = 1;
  }

  private lanesImage(
    activity: Uint8Array[],
    size: { width: number; height: number; ratio: number },
  ): OffscreenCanvas {
    const cached = this.lanes;
    if (
      cached &&
      cached.activity === activity &&
      cached.width === size.width &&
      cached.height === size.height
    ) {
      return cached.image;
    }
    const image = new OffscreenCanvas(size.width, size.height);
    const ctx = image.getContext('2d')!;
    const lane = size.height / activity.length;
    ctx.fillStyle = this.palette.light;
    activity.forEach((levels, i) => {
      const n = levels.length;
      const top = Math.round(i * lane);
      const height = Math.max(1, Math.round((i + 1) * lane) - top - size.ratio);
      for (let x = 0; x < size.width; x++) {
        const first = Math.floor((x / size.width) * n);
        const last = Math.max(first + 1, Math.floor(((x + 1) / size.width) * n));
        let level = 0;
        for (let k = first; k < Math.min(last, n); k++) level = Math.max(level, levels[k]);
        if (level <= LANE_FLOOR) continue;
        ctx.globalAlpha = Math.min((level - LANE_FLOOR) / 150, 1);
        ctx.fillRect(x, top, 1, height);
      }
    });
    ctx.globalAlpha = 1;
    this.lanes = { activity, width: size.width, height: size.height, image };
    return image;
  }

  /** Scrolling view centred on `positionS`, `spanS` of the track wide (`ownSpanS` at its own
   * tempo); without a waveform yet, everything but the audio. */
  drawZoomed(
    ctx: CanvasRenderingContext2D,
    w: Waveform | null,
    overlay: WaveformOverlay,
    positionS: number,
    spanS: number,
    ownSpanS: number,
    size: { width: number; height: number; ratio: number },
  ) {
    const view: View = { fromS: positionS - spanS / 2, spanS, ...size };
    ctx.clearRect(0, 0, view.width, view.height);
    this.drawLoop(ctx, overlay.loop, view);
    if (w) this.drawZoomedAudio(ctx, w, view, view.width / ownSpanS);
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
    if (w) {
      const { full, played } = this.overviewAudio(w, view);
      const split = Math.min(Math.max(Math.round(playedX), 0), view.width);
      if (split > 0) ctx.drawImage(played, 0, 0, split, view.height, 0, 0, split, view.height);
      if (split < view.width) {
        const rest = view.width - split;
        ctx.drawImage(full, split, 0, rest, view.height, split, 0, rest, view.height);
      }
    }
    this.drawCues(ctx, overlay.cues, view, false);
    ctx.fillStyle = this.palette.light;
    ctx.fillRect(Math.floor(playedX) - view.ratio, 0, 2 * view.ratio, view.height);
  }

  /** The audio from its tiles at `pxPerS`, what has played (left of the playhead) veiled. */
  private drawZoomedAudio(ctx: CanvasRenderingContext2D, w: Waveform, view: View, pxPerS: number) {
    const cached = this.tiles;
    const tiles =
      cached && cached.w === w && cached.pxPerS === pxPerS && cached.height === view.height
        ? cached
        : (this.tiles = { w, pxPerS, height: view.height, images: new Map() });
    const tileS = TILE_PX / pxPerS;
    const first = Math.max(Math.floor(view.fromS / tileS), 0);
    const last = Math.min(
      Math.floor((view.fromS + view.spanS) / tileS),
      Math.floor(w.peak.length / w.slicesPerSecond / tileS),
    );
    for (const k of tiles.images.keys()) {
      if (k < first - 1 || k > last + 1) tiles.images.delete(k);
    }
    const width = (tileS / view.spanS) * view.width;
    for (let k = first; k <= last; k++) {
      let image = tiles.images.get(k);
      if (!image) {
        image = new OffscreenCanvas(TILE_PX, view.height);
        const tile = { ...view, fromS: k * tileS, spanS: tileS, width: TILE_PX };
        this.drawColumns(image.getContext('2d')!, w, tile);
        tiles.images.set(k, image);
      }
      ctx.drawImage(image, xOf(k * tileS, view), 0, width, view.height);
    }
    ctx.globalAlpha = 1 - PLAYED;
    ctx.fillStyle = this.palette.void;
    ctx.fillRect(0, 0, Math.round(view.width / 2), view.height);
    ctx.globalAlpha = 1;
  }

  /** The loudest slice under each column of `view`, in its colour. */
  private drawColumns(ctx: OffscreenCanvasRenderingContext2D, w: Waveform, view: View) {
    const sps = w.slicesPerSecond;
    const slicesPerPx = (view.spanS * sps) / view.width;
    const mid = view.height / 2;
    for (let x = 0; x < view.width; x++) {
      const first = Math.floor(view.fromS * sps + x * slicesPerPx);
      const last = Math.max(first + 1, Math.floor(view.fromS * sps + (x + 1) * slicesPerPx));
      if (last <= 0 || first >= w.peak.length) continue;
      const loudest = this.loudest(w, first, last);
      const h = (w.peak[loudest] / 255) * (mid - 1);
      ctx.fillStyle = this.colorAt(w, loudest);
      ctx.fillRect(x, mid - h, 1, h * 2);
    }
  }

  /** The overview's audio, drawn again only for another waveform or another size. */
  private overviewAudio(
    w: Waveform,
    view: View,
  ): { full: OffscreenCanvas; played: OffscreenCanvas } {
    const cached = this.overview;
    if (cached && cached.w === w && cached.width === view.width && cached.height === view.height) {
      return cached;
    }
    const full = new OffscreenCanvas(view.width, view.height);
    const played = new OffscreenCanvas(view.width, view.height);
    this.drawOverviewAudio(full.getContext('2d')!, w, view, 1);
    this.drawOverviewAudio(played.getContext('2d')!, w, view, PLAYED_OVERVIEW);
    this.overview = { w, width: view.width, height: view.height, full, played };
    return this.overview;
  }

  private drawOverviewAudio(
    ctx: OffscreenCanvasRenderingContext2D,
    w: Waveform,
    view: View,
    alpha: number,
  ) {
    const n = w.peak.length;
    const mid = view.height / 2;
    ctx.globalAlpha = alpha;
    for (let x = 0; x < view.width; x++) {
      const first = Math.floor((x / view.width) * n);
      const last = Math.max(first + 1, Math.floor(((x + 1) / view.width) * n));
      const loudest = this.loudest(w, first, last);
      const h = (w.peak[loudest] / 255) * (mid - 1);
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
