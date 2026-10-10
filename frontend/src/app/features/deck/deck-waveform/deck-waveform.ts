import { Component, DestroyRef, ElementRef, afterNextRender, computed, inject, input, viewChild } from '@angular/core';
import { EngineService } from '../../../core/services/engine.service';
import { WaveformOverlay, WaveformPainter } from './waveform-painter';

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
    // ZOOM_SPAN_S of real time: beatmatched decks show their beats at the same spacing
    const span = ZOOM_SPAN_S * (this.engine.tempoRate(this.deck()) ?? 1);
    this.painter.drawZoomed(zoom.ctx, track.waveform, overlay, position, span, zoom.size);
    this.painter.drawOverview(overview.ctx, track.waveform, overlay, position, track.durationS, overview.size);
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
    return ctx ? { ctx, size: { width: w, height: h } } : null;
  }
}
