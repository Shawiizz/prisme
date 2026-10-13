import {
  Component,
  DestroyRef,
  ElementRef,
  afterNextRender,
  computed,
  inject,
  input,
  viewChild,
} from '@angular/core';
import { EngineService } from '../../../core/services/engine.service';
import { WaveformOverlay, WaveformPainter } from './waveform-painter';
import { TPipe } from '../../../core/i18n/t.pipe';

/** Seconds of real time shown in the scrolling view (and the phase strip). */
export const ZOOM_SPAN_S = 10;
/** A press moving less than this (CSS px) is a click. */
const CLICK_PX = 3;
/** The hand's positions go to the engine at most this often (ms), with the time the
 * pointer was there: the engine replays the hand's path from those times. */
const HAND_EVERY_MS = 4;

/** A press on a waveform, from where it started. */
interface Drag {
  view: 'zoom' | 'overview';
  startX: number;
  /** The track position when the press started (s). */
  startS: number;
  moved: boolean;
  /** When the hand's position last went out (event time, ms). */
  sentAt: number;
}

/**
 * Scrolling waveform around the playhead plus an overview of the whole track. Drawn on
 * every animation frame from the extrapolated position, outside change detection.
 * A click on either jumps there. Either is held and moved like a record: the deck plays
 * through the hand's motion, as a record scratched (the engine jumps after a hand too
 * fast to play, as along the overview of a whole track).
 */
@Component({
  selector: 'app-deck-waveform',
  templateUrl: './deck-waveform.html',
  styleUrl: './deck-waveform.scss',
  imports: [TPipe],
})
export class DeckWaveform {
  readonly deck = input.required<number>();

  private readonly engine = inject(EngineService);
  private readonly zoomCanvas = viewChild.required<ElementRef<HTMLCanvasElement>>('zoom');
  private readonly overviewCanvas = viewChild.required<ElementRef<HTMLCanvasElement>>('overview');
  private readonly painter = new WaveformPainter();
  protected readonly track = computed(() => this.engine.tracks()[this.deck()]);
  private frame = 0;
  private drag: Drag | null = null;

  constructor() {
    const destroyRef = inject(DestroyRef);
    afterNextRender(() => {
      const loop = () => {
        this.draw();
        this.frame = requestAnimationFrame(loop);
      };
      this.frame = requestAnimationFrame(loop);
    });
    destroyRef.onDestroy(() => cancelAnimationFrame(this.frame));
  }

  press(event: PointerEvent, view: Drag['view']) {
    if (!this.track() || event.button !== 0) return;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    let startS = this.engine.livePosition(this.deck()) ?? 0;
    if (view === 'overview') {
      startS = this.overviewTime(event);
      this.engine.seek(this.deck(), startS);
    }
    this.drag = { view, startX: event.clientX, startS, moved: false, sentAt: event.timeStamp };
    this.engine.hand(this.deck(), startS, event.timeStamp); // the hand on the record holds it
  }

  move(event: PointerEvent) {
    const drag = this.drag;
    if (!drag) return;
    drag.moved ||= Math.abs(event.clientX - drag.startX) > CLICK_PX;
    if (!drag.moved || event.timeStamp - drag.sentAt < HAND_EVERY_MS) return;
    drag.sentAt = event.timeStamp;
    const width = (event.currentTarget as HTMLElement).clientWidth;
    const at =
      drag.view === 'overview'
        ? this.overviewTime(event)
        : drag.startS - ((event.clientX - drag.startX) / width) * this.zoomSpan();
    this.engine.hand(this.deck(), at, event.timeStamp);
  }

  release(event: PointerEvent) {
    const drag = this.drag;
    if (!drag) return;
    this.cancel();
    if (drag.view !== 'zoom' || drag.moved) return;
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const fromCentre = (event.clientX - rect.left) / rect.width - 0.5;
    const duration = this.track()?.durationS ?? 0;
    const at = drag.startS + fromCentre * this.zoomSpan();
    this.engine.seek(this.deck(), Math.min(Math.max(at, 0), duration));
  }

  /** The hand off the track: play goes on as before. */
  cancel() {
    if (!this.drag) return;
    this.drag = null;
    this.engine.hand(this.deck(), null);
  }

  private overviewTime(event: PointerEvent): number {
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const share = Math.min(Math.max((event.clientX - rect.left) / rect.width, 0), 1);
    return share * (this.track()?.durationS ?? 0);
  }

  /** Seconds of the track shown in the scrolling view: ZOOM_SPAN_S of real time, so that
   * beatmatched decks show their beats at the same spacing. */
  private zoomSpan(): number {
    return ZOOM_SPAN_S * (this.engine.tempoRate(this.deck()) ?? 1);
  }

  private draw() {
    const track = this.track();
    const zoom = this.fit(this.zoomCanvas().nativeElement);
    const overview = this.fit(this.overviewCanvas().nativeElement);
    if (!zoom || !overview) return;
    if (!track) {
      zoom.ctx.clearRect(0, 0, zoom.size.width, zoom.size.height);
      overview.ctx.clearRect(0, 0, overview.size.width, overview.size.height);
      return;
    }
    const position = this.engine.livePosition(this.deck()) ?? 0;
    const overlay: WaveformOverlay = {
      tempo: track.tempo,
      cues: track.cues,
      loop: this.engine.status()?.decks[this.deck()]?.looping ?? null,
    };
    const span = this.zoomSpan();
    const waveform = this.engine.waveforms()[this.deck()];
    this.painter.drawZoomed(zoom.ctx, waveform, overlay, position, span, zoom.size);
    this.painter.drawOverview(
      overview.ctx,
      waveform,
      overlay,
      position,
      track.durationS,
      overview.size,
    );
  }

  /** Keeps the canvas backing store at the element size (device pixels). */
  private fit(canvas: HTMLCanvasElement) {
    const ratio = window.devicePixelRatio || 1;
    const w = Math.max(1, Math.floor(canvas.clientWidth * ratio));
    const h = Math.max(1, Math.floor(canvas.clientHeight * ratio));
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w;
      canvas.height = h;
    }
    const ctx = canvas.getContext('2d');
    return ctx ? { ctx, size: { width: w, height: h, ratio } } : null;
  }
}
