import { Component, DestroyRef, ElementRef, computed, inject, viewChild } from '@angular/core';
import { EngineService } from '../../../core/services/engine.service';
import { MixerService } from '../../../core/services/mixer.service';
import { driftTime, phaseOffset } from '../../../core/sync';
import { t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';
import { ZOOM_SPAN_S } from '../deck-waveform/deck-waveform';
import { fitToDevicePixels, forgetCanvas } from '../../../shared/device-pixels';
import { Showing, everyFrame } from '../../../shared/frames';
import { palette } from '../../../shared/palette';
import { deckLetter } from '../../../shared/time';
import { Crossfader } from '../../mixer/crossfader/crossfader';

/** Beat offset under which the two decks count as matched. */
const MATCHED_BEATS = 0.02;
/** The offset heard as two kicks rather than one: a sixteenth note. */
const HEARD_BEATS = 1 / 16;
/** How far ahead the drift is told (s). */
const FORECAST_S = 60;

/**
 * Between the first two decks: the beats of one deck at the top and of the other at the
 * bottom, around their playheads and at the scale of the waveforms. Matched, they meet.
 * The software mixer has its crossfader there, between its two channels.
 */
@Component({
  selector: 'app-phase-strip',
  templateUrl: './phase-strip.html',
  styleUrl: './phase-strip.scss',
  imports: [Crossfader, TPipe],
})
export class PhaseStrip {
  private readonly engine = inject(EngineService);
  protected readonly mixer = inject(MixerService);
  private readonly lane = viewChild.required<ElementRef<HTMLCanvasElement>>('lane');
  private readonly palette = palette();
  private readonly showing = new Showing();

  /** The two decks compared: with four decks, the first two that play; else A and B. */
  protected readonly pair = computed<[number, number]>(() => {
    const decks = this.engine.status()?.decks ?? [];
    const playing = decks.flatMap((d, i) => (d.playing ? [i] : []));
    return playing.length >= 2 ? [playing[0], playing[1]] : [0, 1];
  });
  /** The pair by its letters, told when there are more than two decks. */
  protected readonly pairName = computed(() =>
    this.engine.deckIndexes().length > 2 ? this.pair().map(deckLetter).join('·') : '',
  );
  private readonly statuses = computed(() => {
    const decks = this.engine.status()?.decks ?? [];
    return this.pair().map((deck) => decks[deck]);
  });

  /** How far the beats of the second deck are from those of the first. */
  protected readonly offset = computed(() => {
    const [a, b] = this.statuses();
    const beats = a && b ? phaseOffset(b, a) : null;
    if (beats === null || !b.bpm) return '';
    if (Math.abs(beats) < MATCHED_BEATS) return t('Matched');
    const ms = String(Math.round((Math.abs(beats) * 60_000) / b.bpm));
    const deck = deckLetter(this.pair()[1]);
    return beats > 0
      ? t('{deck} ahead by {ms} ms', { deck, ms })
      : t('{deck} behind by {ms} ms', { deck, ms });
  });
  /** When the second deck, at the present tempos, will be heard off the beat of the first. */
  protected readonly drift = computed(() => {
    const [a, b] = this.statuses();
    const seconds = a && b ? driftTime(b, a, HEARD_BEATS) : null;
    if (seconds === null || seconds > FORECAST_S) return '';
    return seconds === 0
      ? t('Off the beat: adjust the pitch')
      : t('Off by 1/16 in {s} s', { s: String(Math.round(seconds)) });
  });

  constructor() {
    everyFrame(() => this.draw());
    inject(DestroyRef).onDestroy(() => forgetCanvas(this.lane().nativeElement));
  }

  private draw() {
    const canvas = this.lane().nativeElement;
    const ratio = window.devicePixelRatio || 1;
    const { width, height } = fitToDevicePixels(canvas);
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
      .with(this.engine.tempoRate(deck));
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
      ctx.fillStyle = n % 4 === 0 ? this.palette.light : this.palette.text2;
      ctx.fillRect(x, top, ratio, bottom - top);
    }
  }
}
