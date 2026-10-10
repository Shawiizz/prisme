import { Component, DestroyRef, ElementRef, afterNextRender, computed, inject, input, viewChild } from '@angular/core';
import { EngineService } from '../../../core/services/engine.service';
import { WaveformPainter } from './waveform-painter';

/** Seconds shown in the scrolling view. */
const ZOOM_SPAN_S = 10;

/**
 * Scrolling waveform around the playhead plus an overview of the whole track. Drawn on
 * every animation frame from the extrapolated position, outside change detection.
 */
@Component({
  selector: 'app-deck-waveform',
  templateUrl: './deck-waveform.html',
  styleUrl: './deck-waveform.scss',
})
export class DeckWaveform {
  readonly deck = input.required<number>();

  private readonly engine = inject(EngineService);
  private readonly zoomCanvas = viewChild.required<ElementRef<HTMLCanvasElement>>('zoom');
  private readonly overviewCanvas = viewChild.required<ElementRef<HTMLCanvasElement>>('overview');
  private readonly painter = new WaveformPainter();
  protected readonly track = computed(() => this.engine.tracks()[this.deck()]);
  private frame = 0;

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

  /** Click on the overview: jump there. */
  seekOverview(event: MouseEvent) {
    const t = this.track();
    if (!t) return;
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    this.engine.seek(this.deck(), ((event.clientX - rect.left) / rect.width) * t.durationS);
  }

  private draw() {
    const track = this.track();
    const zoom = this.fit(this.zoomCanvas().nativeElement);
    const overview = this.fit(this.overviewCanvas().nativeElement);
    if (!zoom || !overview) return;
    if (!track) {
      zoom.ctx.clearRect(0, 0, zoom.w, zoom.h);
      overview.ctx.clearRect(0, 0, overview.w, overview.h);
      return;
    }
    const position = this.engine.livePosition(this.deck()) ?? 0;
    this.painter.drawZoomed(zoom.ctx, track.waveform, track.tempo, position, ZOOM_SPAN_S, zoom.w, zoom.h);
    this.painter.drawOverview(overview.ctx, track.waveform, position, track.durationS, overview.w, overview.h);
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
    return ctx ? { ctx, w, h } : null;
  }
}
