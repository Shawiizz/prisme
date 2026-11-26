import {
  Component,
  DestroyRef,
  ElementRef,
  booleanAttribute,
  computed,
  inject,
  input,
  viewChild,
} from '@angular/core';
import { beatsAt, timeOf } from '../../../core/beat-grid';
import { EngineService } from '../../../core/services/engine.service';
import { MixerService } from '../../../core/services/mixer.service';
import { t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';
import { ZOOM_SPAN_S } from '../deck-waveform/deck-waveform';
import { fitToDevicePixels, forgetCanvas } from '../../../shared/device-pixels';
import { Showing, everyFrame } from '../../../shared/frames';
import { palette } from '../../../shared/palette';
import { deckLetter } from '../../../shared/time';
import { same, sameItems } from '../../../core/same';
import { Crossfader } from '../../mixer/crossfader/crossfader';

/**
 * Between the first two decks: the beats of one deck at the top and of the other at the
 * bottom, around their playheads and at the scale of the waveforms. Matched, they meet.
 * The software mixer has its crossfader there, between its two channels.
 */
@Component({
  selector: 'app-phase-strip',
  templateUrl: './phase-strip.html',
  styleUrl: './phase-strip.scss',
  host: { '[class.across]': 'across()' },
  imports: [Crossfader, TPipe],
})
export class PhaseStrip {
  /** Across the decks' own window, under waveforms as wide: its words over it, the crossfader
   * with the channels. */
  readonly across = input(false, { transform: booleanAttribute });
  private readonly engine = inject(EngineService);
  protected readonly mixer = inject(MixerService);
  private readonly lane = viewChild.required<ElementRef<HTMLCanvasElement>>('lane');
  private readonly palette = palette();
  private readonly showing = new Showing();
  private readonly redraw = () => this.draw();

  /** What the backend tells of the two decks compared. */
  private readonly strip = computed(() => this.engine.status()?.phase ?? null, { equal: same });
  /** The two decks compared: with four decks, the first two that play; else A and B. */
  protected readonly pair = computed<[number, number]>(() => this.strip()?.decks ?? [0, 1], {
    equal: sameItems,
  });
  /** The pair by its letters, told when there are more than two decks. */
  protected readonly pairName = computed(() =>
    this.engine.deckIndexes().length > 2 ? this.pair().map(deckLetter).join('·') : '',
  );

  /**
   * What the strip says of the two decks: drifting apart at their tempos, when the second will
   * be heard off the beat of the first; else how far its beats are from the first's.
   */
  protected readonly phase = computed(() => {
    const strip = this.strip();
    const deck = deckLetter(this.pair()[1]);
    switch (strip?.state) {
      case 'drifting':
        return {
          words:
            strip.seconds === 0
              ? t('Off the beat: adjust the pitch')
              : t('Off by 1/16 in {s} s', { s: String(strip.seconds) }),
          drifting: true,
        };
      case 'matched':
        return { words: t('Matched'), drifting: false };
      case 'ahead':
        return {
          words: t('{deck} ahead by {ms} ms', { deck, ms: String(strip.ms) }),
          drifting: false,
        };
      case 'behind':
        return {
          words: t('{deck} behind by {ms} ms', { deck, ms: String(strip.ms) }),
          drifting: false,
        };
      default:
        return { words: '', drifting: false };
    }
  });

  constructor() {
    everyFrame(this.redraw);
    inject(DestroyRef).onDestroy(() => forgetCanvas(this.lane().nativeElement));
  }

  private draw() {
    const canvas = this.lane().nativeElement;
    const ratio = window.devicePixelRatio || 1;
    const { width, height } = fitToDevicePixels(canvas, this.redraw);
    const [first, second] = this.pair();
    this.showing.start().with(width).with(height);
    this.show(first);
    this.show(second);
    if (!this.showing.isNew()) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.clearRect(0, 0, width, height);
    const mid = Math.round(height / 2);
    const gap = 3 * ratio;
    this.drawBeats(ctx, first, width, gap, mid - ratio, ratio);
    this.drawBeats(ctx, second, width, mid + ratio, height - gap, ratio);
    ctx.fillStyle = this.palette.line;
    ctx.fillRect(0, mid, width, ratio);
    ctx.fillStyle = this.palette.light;
    ctx.fillRect(Math.round(width / 2) - ratio, 0, 2 * ratio, height);
  }

  /** What the beats of `deck` are drawn from. */
  private show(deck: number) {
    this.showing
      .with(this.engine.tracks()[deck]?.tempo)
      .with(this.engine.livePosition(deck))
      .with(this.rate(deck));
  }

  private rate(deck: number): number {
    return this.engine.deckStatus(deck)?.rate ?? 1;
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
    const span = ZOOM_SPAN_S * this.rate(deck);
    const from = position - span / 2;
    const first = Math.ceil(beatsAt(tempo, Math.max(from, 0)));
    const last = Math.floor(beatsAt(tempo, from + span));
    const bar = tempo.beatsPerBar;
    for (let n = first; n <= last; n++) {
      const x = Math.round(((timeOf(tempo, n) - from) / span) * width);
      ctx.fillStyle = ((n % bar) + bar) % bar === 0 ? this.palette.light : this.palette.text2;
      ctx.fillRect(x, top, ratio, bottom - top);
    }
  }
}
