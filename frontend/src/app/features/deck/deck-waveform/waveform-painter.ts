import { Cue, LoopStatus, Tempo, Waveform } from '../../../core/models/engine';
import { palette } from '../../../shared/palette';
import { hexColor } from '../../../shared/track-colors';

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
/** Width of the tiles the scrolling view's audio is drawn in once (device pixels). */
const TILE_PX = 1024;
/** Share of the half height the loudest slices reach in the scrolling view: the rest holds the
 * beat grid's ticks and markers. */
const HEADROOM = 0.88;
/** Opacity of a beat's line through the waveform, of its ticks at the edges, and of a bar's
 * line. */
const GRID_BEAT = 0.14;
const GRID_TICK = 0.6;
const GRID_BAR = 0.5;
/** The colours of slices kept written at most: a long set draws many, the next written again. */
const COLORS_KEPT = 4096;

/**
 * Draws the waveforms (design/DESIGN.md, Waveform): the envelope the backend made of each slice,
 * in its colour; the beat grid over it, discreet (the bars in red, marked at both edges); cues as flags in their tag
 * colour, the loop as a band of light and the playhead, the only white line, dispersed on its
 * edges.
 */
export class WaveformPainter {
  private readonly palette = palette();
  /** The colour of each tag of a cue, written once. */
  private readonly cueColors = new Map<number, string>();
  /** The colours of the slices drawn, by their red, green and blue, each written once. */
  private readonly colors = new Map<number, string>();
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
        let shown = 0;
        for (let k = first; k < Math.min(last, n); k++) shown = Math.max(shown, levels[k]);
        if (shown === 0) continue;
        ctx.globalAlpha = shown / 255;
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
    const durationS = w && w.slices / w.slicesPerSecond;
    this.drawRolls(ctx, durationS, view);
    if (overlay.tempo) this.drawBeatGrid(ctx, overlay.tempo, durationS, view);
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

  /** The audio from its tiles at `pxPerS`, what has played (left of the playhead) dimmed. A
   * waveform growing keeps the tiles of the slices that no longer change. */
  private drawZoomedAudio(ctx: CanvasRenderingContext2D, w: Waveform, view: View, pxPerS: number) {
    const tileS = TILE_PX / pxPerS;
    const cached = this.tiles;
    const kept = cached && cached.pxPerS === pxPerS && cached.height === view.height;
    let tiles = cached!;
    if (!kept || (cached.w !== w && w.height.buffer !== cached.w.height.buffer)) {
      tiles = this.tiles = { w, pxPerS, height: view.height, images: new Map() };
    } else if (cached.w !== w) {
      // the tiles drawn hold the slices before where the waveform drawn last changed, final
      const stale = Math.floor(cached.w.changedFrom / w.slicesPerSecond / tileS);
      for (const k of tiles.images.keys()) if (k >= stale) tiles.images.delete(k);
      tiles.w = w;
    }
    const first = Math.max(Math.floor(view.fromS / tileS), 0);
    const last = Math.min(
      Math.floor((view.fromS + view.spanS) / tileS),
      Math.floor(w.slices / w.slicesPerSecond / tileS),
    );
    for (const k of tiles.images.keys()) {
      if (k < first - 1 || k > last + 1) tiles.images.delete(k);
    }
    for (let k = first; k <= last; k++) {
      if (tiles.images.has(k)) continue;
      const image = new OffscreenCanvas(TILE_PX, view.height);
      const tile = { ...view, fromS: k * tileS, spanS: tileS, width: TILE_PX };
      this.drawColumns(image.getContext('2d')!, w, tile, (view.height / 2) * HEADROOM);
      tiles.images.set(k, image);
    }
    // whole columns copied, not blended across pixels as they scroll
    ctx.imageSmoothingEnabled = false;
    const playhead = Math.round(view.width / 2);
    // what has played (left of the playhead) dimmed, then what is to play
    for (let side = 0; side < 2; side++) {
      const from = side === 0 ? 0 : playhead;
      const to = side === 0 ? playhead : view.width;
      ctx.save();
      ctx.beginPath();
      ctx.rect(from, 0, to - from, view.height);
      ctx.clip();
      ctx.globalAlpha = side === 0 ? PLAYED : 1;
      // each tile from its left edge to the next one's, rounded alike: no seam between them
      for (let k = first; k <= last; k++) {
        const x0 = Math.round(xOf(k * tileS, view));
        const x1 = Math.round(xOf((k + 1) * tileS, view));
        ctx.drawImage(tiles.images.get(k)!, x0, 0, x1 - x0, view.height);
      }
      ctx.restore();
    }
  }

  /** The highest slice under each column of `view`, in its colour, the highest of the track
   * `reach` high. */
  private drawColumns(
    ctx: OffscreenCanvasRenderingContext2D,
    w: Waveform,
    view: View,
    reach: number,
  ) {
    const sps = w.slicesPerSecond;
    const slicesPerPx = (view.spanS * sps) / view.width;
    const mid = view.height / 2;
    for (let x = 0; x < view.width; x++) {
      const first = Math.floor(view.fromS * sps + x * slicesPerPx);
      const last = Math.max(first + 1, Math.floor(view.fromS * sps + (x + 1) * slicesPerPx));
      if (last <= 0 || first >= w.height.length) continue;
      const highest = this.highest(w, first, last);
      const h = (w.height[highest] / 255) * reach;
      ctx.fillStyle = this.color(w, highest);
      ctx.fillRect(x, mid - h, 1, h * 2);
    }
  }

  /** The overview's audio, drawn again only for another waveform or another size, on the
   * canvases of before when they have its size. */
  private overviewAudio(
    w: Waveform,
    view: View,
  ): { full: OffscreenCanvas; played: OffscreenCanvas } {
    const cached = this.overview;
    const sized = cached && cached.width === view.width && cached.height === view.height;
    if (sized && cached.w === w) return cached;
    const full = sized ? cached.full : new OffscreenCanvas(view.width, view.height);
    const played = sized ? cached.played : new OffscreenCanvas(view.width, view.height);
    const ctx = full.getContext('2d')!;
    ctx.clearRect(0, 0, view.width, view.height);
    this.drawOverviewAudio(ctx, w, view);
    const dimmed = played.getContext('2d')!;
    dimmed.clearRect(0, 0, view.width, view.height);
    dimmed.globalAlpha = PLAYED_OVERVIEW;
    dimmed.drawImage(full, 0, 0);
    this.overview = { w, width: view.width, height: view.height, full, played };
    return this.overview;
  }

  /** Each column as high as the mean of its slices, so that the parts of the track read by
   * their loudness, the loudest column across the whole height; in the colour of its highest
   * slice. */
  private drawOverviewAudio(ctx: OffscreenCanvasRenderingContext2D, w: Waveform, view: View) {
    const n = w.slices;
    const known = w.height.length;
    const means = new Float32Array(view.width);
    for (let x = 0; x < view.width; x++) {
      const first = Math.floor((x / view.width) * n);
      const last = Math.min(Math.max(first + 1, Math.floor(((x + 1) / view.width) * n)), known);
      let sum = 0;
      for (let i = first; i < last; i++) sum += w.height[i];
      means[x] = last > first ? sum / (last - first) : 0;
    }
    const highest = Math.max(...means);
    if (highest === 0) return;
    const mid = view.height / 2;
    for (let x = 0; x < view.width; x++) {
      const first = Math.floor((x / view.width) * n);
      const last = Math.max(first + 1, Math.floor(((x + 1) / view.width) * n));
      if (first >= known) break;
      const h = (means[x] / highest) * (mid - 1);
      ctx.fillStyle = this.color(w, this.highest(w, first, last));
      ctx.fillRect(x, mid - h, 1, h * 2);
    }
  }

  /** The highest slice from `first` to `last` (excluded), within the slices known. */
  private highest(w: Waveform, first: number, last: number): number {
    let best = Math.max(first, 0);
    for (let i = best; i < Math.min(last, w.height.length); i++) {
      if (w.height[i] > w.height[best]) best = i;
    }
    return best;
  }

  /** Before the track and after it, outlined arrows pointing to it: they scroll by as the
   * silence there plays (a record's lead-in). */
  private drawRolls(ctx: CanvasRenderingContext2D, durationS: number | null, view: View) {
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
  private drawBeatGrid(
    ctx: CanvasRenderingContext2D,
    tempo: Tempo,
    durationS: number | null,
    view: View,
  ) {
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

  /** The colour of slice `k` of `w`. */
  private color(w: Waveform, k: number): string {
    const rgb = (w.red[k] << 16) | (w.green[k] << 8) | w.blue[k];
    let written = this.colors.get(rgb);
    if (written === undefined) {
      if (this.colors.size >= COLORS_KEPT) this.colors.clear();
      written = `rgb(${w.red[k]},${w.green[k]},${w.blue[k]})`;
      this.colors.set(rgb, written);
    }
    return written;
  }
}

function xOf(seconds: number, view: View): number {
  return ((seconds - view.fromS) / view.spanS) * view.width;
}
