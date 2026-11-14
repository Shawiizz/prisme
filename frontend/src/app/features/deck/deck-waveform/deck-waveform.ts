import {
  Component,
  DestroyRef,
  ElementRef,
  afterNextRender,
  computed,
  effect,
  inject,
  input,
  signal,
  viewChild,
} from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { PHONE } from '../../../core/platform';
import { StemsService } from '../../../core/services/stems.service';
import { t } from '../../../core/i18n/i18n';
import { fitToDevicePixels, forgetCanvas } from '../../../shared/device-pixels';
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
/** Track time in one turn of a jog wheel (as the engine counts it, `jog.rs`). */
const JOG_TURN_S = 1.8;
/** On a phone, a finger held still this long (ms) puts the hand on the record. */
const HOLD_MS = 280;

/** A press on a waveform, from where it started. */
interface Drag {
  view: 'zoom' | 'overview';
  startX: number;
  /** The track position when the press started (s). */
  startS: number;
  moved: boolean;
  /** When the hand's position last went out (event time, ms). */
  sentAt: number;
  /** On a phone: the finger slides the edge of the jog, or holds the record (after a
   * moment still); `null` until it is known. */
  touch: 'edge' | 'hand' | null;
  /** Where the edge was last turned from (CSS px). */
  lastX: number;
  /** The hand is on the record: it lets go with the press. */
  held: boolean;
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
  private readonly lanesCanvas = viewChild<ElementRef<HTMLCanvasElement>>('lanes');
  private readonly stems = inject(StemsService);
  private readonly painter = new WaveformPainter();
  protected readonly track = computed(() => this.engine.tracks()[this.deck()]);
  /** Where each part of the separated track plays (`stem_activity`), once it is all
   * separated. */
  protected readonly activity = signal<Uint8Array[] | null>(null);
  /** The load and separation the activity was asked for. */
  private activityOf = '';
  protected readonly lanesTitle = computed(() =>
    this.activity()?.length === 4
      ? t('Vocals, drums, bass and melody along the track')
      : t('Vocals and instrumental along the track'),
  );
  private frame = 0;
  private drag: Drag | null = null;
  /** On a phone, until the finger has been still long enough to hold the record. */
  private holding: ReturnType<typeof setTimeout> | null = null;
  /** What the last frame showed: a frame showing the same is not drawn again (a deck at
   * rest costs nothing). */
  private shown: unknown[] = [];

  constructor() {
    const destroyRef = inject(DestroyRef);
    afterNextRender(() => {
      const loop = () => {
        this.draw();
        this.frame = requestAnimationFrame(loop);
      };
      this.frame = requestAnimationFrame(loop);
    });
    destroyRef.onDestroy(() => {
      cancelAnimationFrame(this.frame);
      forgetCanvas(this.zoomCanvas().nativeElement);
      forgetCanvas(this.overviewCanvas().nativeElement);
    });
    effect(() => this.askActivity());
    // the lanes' canvas comes and goes with the stems
    effect((onCleanup) => {
      const lanes = this.lanesCanvas()?.nativeElement;
      if (lanes) onCleanup(() => forgetCanvas(lanes));
    });
  }

  /** Asks the activity of the track once it is separated, again until it is all there. */
  private askActivity() {
    const track = this.track();
    const separating = this.stems.progress().current === track?.path;
    const of =
      track && track.separation !== 'none' && !separating
        ? `${track.loadId} ${track.separation}`
        : '';
    if (of === this.activityOf) return;
    this.activityOf = of;
    this.activity.set(null);
    if (!track || !of) return;
    const parts = track.separation === 'full' ? 4 : 2;
    invoke<ArrayBuffer>('stem_activity', { deck: this.deck(), loadId: track.loadId }).then(
      (buffer) => {
        if (this.activityOf !== of) return;
        const bytes = new Uint8Array(buffer);
        const slices = bytes.length / parts;
        if (!slices) {
          this.activityOf = '';
          return;
        }
        this.activity.set(
          Array.from({ length: parts }, (_, i) => bytes.subarray(i * slices, (i + 1) * slices)),
        );
      },
    );
  }

  press(event: PointerEvent, view: Drag['view']) {
    if (!this.track() || event.button !== 0) return;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    let startS = this.engine.livePosition(this.deck()) ?? 0;
    if (view === 'overview') {
      startS = this.overviewTime(event);
      this.engine.seek(this.deck(), startS);
    }
    const jog = PHONE && view === 'zoom';
    this.drag = {
      view,
      startX: event.clientX,
      startS,
      moved: false,
      sentAt: event.timeStamp,
      touch: null,
      lastX: event.clientX,
      held: !jog,
    };
    if (jog) {
      // a finger held still takes the record; one that slides at once turns the edge
      this.holding = setTimeout(() => this.holdRecord(), HOLD_MS);
      return;
    }
    this.engine.hand(this.deck(), startS, event.timeStamp); // the hand on the record holds it
  }

  move(event: PointerEvent) {
    const drag = this.drag;
    if (!drag) return;
    drag.moved ||= Math.abs(event.clientX - drag.startX) > CLICK_PX;
    if (drag.moved && drag.touch === null && drag.view === 'zoom' && PHONE) {
      this.stopHolding();
      drag.touch = 'edge';
    }
    if (drag.touch === 'edge') {
      this.turnEdge(drag, event);
      return;
    }
    if (drag.touch === null && PHONE && drag.view === 'zoom') return;
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
    // a finger on a phone never makes the track jump: it might be on it by mistake
    if (drag.view !== 'zoom' || drag.moved || PHONE) return;
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const fromCentre = (event.clientX - rect.left) / rect.width - 0.5;
    const duration = this.track()?.durationS ?? 0;
    const at = drag.startS + fromCentre * this.zoomSpan();
    this.engine.seek(this.deck(), Math.min(Math.max(at, 0), duration));
  }

  /** The hand off the track: play goes on as before. */
  cancel() {
    const drag = this.drag;
    if (!drag) return;
    this.stopHolding();
    this.drag = null;
    if (drag.held) this.engine.hand(this.deck(), null);
  }

  /** The finger has stayed still: it holds the record, and moves it from now on. */
  private holdRecord() {
    this.holding = null;
    const drag = this.drag;
    if (!drag || drag.touch !== null) return;
    drag.touch = 'hand';
    drag.held = true;
    drag.startS = this.engine.livePosition(this.deck()) ?? drag.startS;
    drag.moved = true;
    this.engine.hand(this.deck(), drag.startS);
  }

  private stopHolding() {
    if (this.holding !== null) clearTimeout(this.holding);
    this.holding = null;
  }

  /** The finger slides the edge of the jog: the waveform follows it as far as the track
   * goes, pushing a playing track for a moment, moving a stopped one. */
  private turnEdge(drag: Drag, event: PointerEvent) {
    const width = (event.currentTarget as HTMLElement).clientWidth;
    const seconds = -((event.clientX - drag.lastX) / width) * this.zoomSpan();
    drag.lastX = event.clientX;
    if (seconds !== 0) this.engine.jog(this.deck(), seconds / JOG_TURN_S, event.timeStamp);
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
    const lanesCanvas = this.lanesCanvas()?.nativeElement;
    const lanes = lanesCanvas ? this.fit(lanesCanvas) : null;
    const activity = this.activity();
    if (!zoom || !overview) return;
    if (!track) {
      zoom.ctx.clearRect(0, 0, zoom.size.width, zoom.size.height);
      overview.ctx.clearRect(0, 0, overview.size.width, overview.size.height);
      this.shown = [];
      return;
    }
    const position = this.engine.livePosition(this.deck()) ?? 0;
    const loop = this.engine.status()?.decks[this.deck()]?.looping ?? null;
    const overlay: WaveformOverlay = { tempo: track.tempo, cues: track.cues, loop };
    const span = this.zoomSpan();
    const waveform = this.engine.waveforms()[this.deck()];
    const showing = [
      track,
      waveform,
      position,
      span,
      loop?.active,
      loop?.loopInS,
      loop?.startS,
      loop?.endS,
      zoom.size.width,
      zoom.size.height,
      overview.size.width,
      overview.size.height,
      activity,
      lanes?.size.width,
      lanes?.size.height,
    ];
    if (showing.length === this.shown.length && showing.every((v, i) => v === this.shown[i]))
      return;
    this.shown = showing;
    this.painter.drawZoomed(zoom.ctx, waveform, overlay, position, span, ZOOM_SPAN_S, zoom.size);
    this.painter.drawOverview(
      overview.ctx,
      waveform,
      overlay,
      position,
      track.durationS,
      overview.size,
    );
    if (lanes && activity) {
      this.painter.drawStemLanes(lanes.ctx, activity, position, track.durationS, lanes.size);
    }
  }

  /** The canvas at its size in device pixels, and its context; nothing until it is laid out. */
  private fit(canvas: HTMLCanvasElement) {
    const { width, height } = fitToDevicePixels(canvas);
    const ctx = canvas.getContext('2d');
    const laidOut = width > 0 && height > 0;
    return ctx && laidOut
      ? { ctx, size: { width, height, ratio: window.devicePixelRatio || 1 } }
      : null;
  }
}
