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
import { MixerService } from '../../../core/services/mixer.service';
import { PreviewService } from '../../../core/services/preview.service';
import { TPipe } from '../../../core/i18n/t.pipe';
import { fitToDevicePixels, forgetCanvas } from '../../../shared/device-pixels';
import { Knob } from '../../../shared/knob/knob';
import { readout } from '../../../shared/readout';
import { Showing, everyFrame } from '../../../shared/frames';
import { formatClock } from '../../../shared/time';
import { WaveformOverlay, WaveformPainter } from '../../deck/deck-waveform/waveform-painter';
import type { KnobState } from '../../../core/models/backend';

const NOTHING_OVER: WaveformOverlay = { tempo: null, cues: [], loop: null };

/**
 * The track previewed in the headphones: its title and artist, where it plays, the whole
 * track to point at (its waveform once known, as the decks draw it), the headphones' volume
 * (which no mixer on the screen shows with an external one) and a key that stops it. Under
 * the library, on a computer and on a phone.
 */
@Component({
  selector: 'app-preview-strip',
  templateUrl: './preview-strip.html',
  styleUrl: './preview-strip.scss',
  host: { '[class.phone]': 'phone()' },
  imports: [Knob, TPipe],
})
export class PreviewStrip {
  /** Laid out for a finger. */
  readonly phone = input(false, { transform: booleanAttribute });

  protected readonly preview = inject(PreviewService);
  protected readonly mixer = inject(MixerService);
  protected readonly read = (knob: KnobState) => readout(knob.reads);
  private readonly canvas = viewChild.required<ElementRef<HTMLCanvasElement>>('bar');
  private readonly painter = new WaveformPainter();
  private readonly showing = new Showing();
  private readonly redraw = () => this.draw();
  /** The finger or button seeking, while it presses. */
  private pointer: number | null = null;

  protected readonly time = computed(() => {
    const status = this.preview.status();
    if (!status) return '';
    return `${formatClock(status.positionS)} / ${formatClock(status.durationS)}`;
  });

  constructor() {
    everyFrame(this.redraw);
    inject(DestroyRef).onDestroy(() => forgetCanvas(this.canvas().nativeElement));
  }

  protected press(event: PointerEvent) {
    if (this.pointer !== null || event.button !== 0) return;
    this.pointer = event.pointerId;
    (event.currentTarget as Element).setPointerCapture(event.pointerId);
    this.seekTo(event);
  }

  protected move(event: PointerEvent) {
    if (event.pointerId === this.pointer) this.seekTo(event);
  }

  protected release(event: PointerEvent) {
    if (event.pointerId === this.pointer) this.pointer = null;
  }

  private seekTo(event: PointerEvent) {
    const r = (event.currentTarget as Element).getBoundingClientRect();
    this.preview.seek(Math.min(Math.max((event.clientX - r.left) / r.width, 0), 1));
  }

  private draw() {
    const canvas = this.canvas().nativeElement;
    const { width, height } = fitToDevicePixels(canvas, this.redraw);
    const ctx = canvas.getContext('2d');
    const status = this.preview.status();
    const waveform = this.preview.waveform();
    if (!ctx || width === 0 || height === 0) {
      // hidden, its canvas emptied: drawn again once shown
      this.showing.forget();
      return;
    }
    this.showing
      .start()
      .with(status?.positionS)
      .with(status?.durationS)
      .with(waveform)
      .with(width)
      .with(height);
    if (!this.showing.isNew()) return;
    if (!status) {
      ctx.clearRect(0, 0, width, height);
      return;
    }
    const size = { width, height, ratio: window.devicePixelRatio || 1 };
    this.painter.drawOverview(
      ctx,
      waveform,
      NOTHING_OVER,
      status.positionS,
      status.durationS,
      size,
    );
  }
}
