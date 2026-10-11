import {
  Component,
  DestroyRef,
  ElementRef,
  afterNextRender,
  computed,
  inject,
  viewChild,
} from '@angular/core';
import { EngineService } from '../../../core/services/engine.service';
import { phaseOffset } from '../../../core/sync';
import { t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';
import { ZOOM_SPAN_S } from '../deck-waveform/deck-waveform';

/** Beat offset under which the two decks count as matched. */
const MATCHED_BEATS = 0.02;

/**
 * Between the two decks: the beats of A at the top and of B at the bottom, around their
 * playheads and at the scale of the waveforms. Matched, they meet.
 */
@Component({
  selector: 'app-phase-strip',
  templateUrl: './phase-strip.html',
  styleUrl: './phase-strip.scss',
  imports: [TPipe],
})
export class PhaseStrip {
  private readonly engine = inject(EngineService);
  private readonly lane = viewChild.required<ElementRef<HTMLCanvasElement>>('lane');
  private readonly style = getComputedStyle(document.documentElement);
  private readonly colors = {
    light: this.token('--pr-light'),
    beat: this.token('--pr-text-2'),
    line: this.token('--pr-line'),
  };
  private frame = 0;

  /** How far B's beats are from A's. */
  protected readonly offset = computed(() => {
    const [a, b] = this.engine.status()?.decks ?? [];
    const beats = a && b ? phaseOffset(b, a) : null;
    if (beats === null || !b.bpm) return '';
    if (Math.abs(beats) < MATCHED_BEATS) return t('Matched');
    const ms = String(Math.round((Math.abs(beats) * 60_000) / b.bpm));
    return beats > 0 ? t('B ahead by {ms} ms', { ms }) : t('B behind by {ms} ms', { ms });
  });

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

  private draw() {
    const canvas = this.lane().nativeElement;
    const ratio = window.devicePixelRatio || 1;
    const width = Math.max(1, Math.floor(canvas.clientWidth * ratio));
    const height = Math.max(1, Math.floor(canvas.clientHeight * ratio));
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
    }
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.clearRect(0, 0, width, height);
    const mid = Math.round(height / 2);
    const gap = 3 * ratio;
    this.drawBeats(ctx, 0, width, gap, mid - ratio, ratio);
    this.drawBeats(ctx, 1, width, mid + ratio, height - gap, ratio);
    ctx.fillStyle = this.colors.line;
    ctx.fillRect(0, mid, width, ratio);
    ctx.fillStyle = this.colors.light;
    ctx.fillRect(Math.round(width / 2) - ratio, 0, 2 * ratio, height);
  }

  /** The beats of `deck` around its playhead, between `top` and `bottom`. */
  private drawBeats(
    ctx: CanvasRenderingContext2D,
    deck: number,
    width: number,
    top: number,
    bottom: number,
    ratio: number,
  ) {
    const tempo = this.engine.tracks()[deck]?.tempo;
    const position = this.engine.livePosition(deck);
    if (!tempo || position === null) return;
    const span = ZOOM_SPAN_S * (this.engine.tempoRate(deck) ?? 1);
    const period = 60 / tempo.bpm;
    const from = position - span / 2;
    const first = Math.max(0, Math.ceil((from - tempo.firstBeatS) / period));
    const last = Math.floor((from + span - tempo.firstBeatS) / period);
    for (let n = first; n <= last; n++) {
      const x = Math.round(((tempo.firstBeatS + n * period - from) / span) * width);
      ctx.fillStyle = n % 4 === 0 ? this.colors.light : this.colors.beat;
      ctx.fillRect(x, top, ratio, bottom - top);
    }
  }

  private token(name: string): string {
    return this.style.getPropertyValue(name).trim();
  }
}
