import {
  Component,
  DestroyRef,
  ElementRef,
  booleanAttribute,
  computed,
  effect,
  inject,
  input,
  signal,
  untracked,
  viewChild,
} from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { t } from '../../../core/i18n/i18n';
import { fitToDevicePixels, forgetCanvas } from '../../../shared/device-pixels';
import { Showing, everyFrame } from '../../../shared/frames';
import { EngineService } from '../../../core/services/engine.service';
import { WaveformOverlay, WaveformPainter } from './waveform-painter';
import { TPipe } from '../../../core/i18n/t.pipe';

/** Seconds of real time shown in the scrolling view (and the phase strip). */
export const ZOOM_SPAN_S = 10;
/** A press moving less than this (CSS px) has not moved: on the scrolling view, it does nothing. */
const CLICK_PX = 3;
/** The hand's positions go to the engine at most this often (ms), with the time the
 * pointer was there: the engine replays the hand's path from those times. */
const HAND_EVERY_MS = 4;
/** A slide of the edge moves the track this share of the waveform slid over: lining up beats
 * is fine work. */
const EDGE_SHARE = 0.2;
/** A press held still this long (ms) puts the hand on the record. */
const HOLD_MS = 280;

/** A press on a waveform, from where it started. */
interface Drag {
  /** The finger or button that pressed: another one does nothing meanwhile. */
  pointer: number;
  /** The deck pressed: the one let go, even once the waveform shows another. */
  deck: number;
  view: 'zoom' | 'overview';
  startX: number;
  /** The track position when the press started (s). */
  startS: number;
  moved: boolean;
  /** When the hand's position last went out (event time, ms). */
  sentAt: number;
  /** A press on the scrolling view: it slides the edge of the jog, or holds the record. */
  jog: boolean;
  /** What the press does: slides the edge, or holds the record (still a moment, then moved);
   * `null` until it is known. */
  touch: 'edge' | 'hand' | null;
  /** Where the edge was last turned from (CSS px). */
  lastX: number;
  /** The hand is on the record: it lets go with the press. */
  held: boolean;
  /** Where the press puts the track, for a hand on the record or the edge of a stopped one:
   * drawn there at once, at the screen's rate, rather than where the engine last said. */
  shownS: number | null;
}

/**
 * Scrolling waveform around the playhead plus an overview of the whole track. Drawn on
 * every animation frame from the extrapolated position, outside change detection.
 * On the scrolling view only a slide acts, never a press that stays still: slid at once, it
 * nudges the track as the edge of a jog; held still a moment and then slid, it moves the track
 * like a record. A press on the overview jumps there and moves the track like a record at
 * once: the deck plays through the hand's motion, as a record scratched (the engine jumps after
 * a hand too fast to play, as along the overview of a whole track).
 */
@Component({
  selector: 'app-deck-waveform',
  templateUrl: './deck-waveform.html',
  styleUrl: './deck-waveform.scss',
  host: { '[class.compact]': 'compact()' },
  imports: [TPipe],
})
export class DeckWaveform {
  readonly deck = input.required<number>();
  /** Half the height, for four decks. */
  readonly compact = input(false, { transform: booleanAttribute });

  private readonly engine = inject(EngineService);
  private readonly zoomCanvas = viewChild.required<ElementRef<HTMLCanvasElement>>('zoom');
  private readonly overviewCanvas = viewChild.required<ElementRef<HTMLCanvasElement>>('overview');
  private readonly lanesCanvas = viewChild<ElementRef<HTMLCanvasElement>>('lanes');
  private readonly painter = new WaveformPainter();
  protected readonly track = computed(() => this.engine.tracks()[this.deck()]);
  /** In passthrough, the inputs the deck plays instead of its track (`3-4`): its own pair, or
   * that of the deck it is layered over on a device without a pair for each deck. */
  protected readonly through = computed(() => {
    const { passthrough, input } = this.engine.decks()[this.deck()];
    return passthrough && input !== null ? `${input + 1}-${input + 2}` : null;
  });
  /** How strongly each part of the separated track shows along it (`stem_activity`), once
   * it is all separated. */
  protected readonly activity = signal<Uint8Array[] | null>(null);
  /** The load and stems the activity was asked for. */
  private activityOf = '';
  protected readonly lanesTitle = computed(() =>
    this.activity()?.length === 4
      ? t('Vocals, drums, bass and melody along the track')
      : t('Vocals and instrumental along the track'),
  );
  private drag: Drag | null = null;
  /** A finger, until it has been still long enough to hold the record. */
  private holding: ReturnType<typeof setTimeout> | null = null;
  private readonly showing = new Showing();
  private readonly redraw = () => this.draw();

  constructor() {
    everyFrame(this.redraw);
    inject(DestroyRef).onDestroy(() => {
      this.cancel();
      forgetCanvas(this.zoomCanvas().nativeElement);
      forgetCanvas(this.overviewCanvas().nativeElement);
    });
    effect(() => this.askActivity());
    // another deck shown: the press on the one before ends
    effect(() => {
      this.deck();
      untracked(() => this.cancel());
    });
    // the lanes' canvas comes and goes with the stems
    effect((onCleanup) => {
      const lanes = this.lanesCanvas()?.nativeElement;
      if (lanes) onCleanup(() => forgetCanvas(lanes));
    });
  }

  /** Asks the activity of the track again whenever its stems change: none until they are all
   * there. */
  private askActivity() {
    const track = this.track();
    const of = track ? `${track.loadId} ${track.separation} ${track.partial}` : '';
    if (of === this.activityOf) return;
    this.activityOf = of;
    this.activity.set(null);
    if (!track) return;
    invoke<ArrayBuffer>('stem_activity', { deck: this.deck(), loadId: track.loadId }).then(
      (buffer) => {
        const bytes = new Uint8Array(buffer);
        const parts = bytes[0] ?? 0;
        if (this.activityOf !== of || !parts) return;
        const slices = (bytes.length - 1) / parts;
        this.activity.set(
          Array.from({ length: parts }, (_, i) =>
            bytes.subarray(1 + i * slices, 1 + (i + 1) * slices),
          ),
        );
      },
      () => {},
    );
  }

  press(event: PointerEvent, view: Drag['view']) {
    if (!this.track() || event.button !== 0 || this.drag) return;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    let startS = this.engine.livePosition(this.deck()) ?? 0;
    if (view === 'overview') {
      startS = this.overviewTime(event);
      this.engine.seek(this.deck(), startS);
    }
    const jog = view === 'zoom';
    this.drag = {
      pointer: event.pointerId,
      deck: this.deck(),
      view,
      startX: event.clientX,
      startS,
      moved: false,
      sentAt: event.timeStamp,
      jog,
      touch: null,
      lastX: event.clientX,
      held: !jog,
      shownS: null,
    };
    if (jog) {
      // held still, it takes the record; slid at once, it turns the edge
      this.holding = setTimeout(() => this.holdRecord(), HOLD_MS);
      return;
    }
    this.engine.hand(this.deck(), startS, event.timeStamp); // the hand on the record holds it
  }

  move(event: PointerEvent) {
    const drag = this.drag;
    if (drag?.pointer !== event.pointerId) return;
    drag.moved ||= Math.abs(event.clientX - drag.startX) > CLICK_PX;
    if (drag.jog && drag.moved && drag.touch === null) {
      this.stopHolding();
      drag.touch = 'edge';
    }
    if (drag.touch === 'edge') {
      this.turnEdge(drag, event);
      return;
    }
    if (drag.jog && drag.touch === null) return;
    if (!drag.moved) return;
    if (!drag.held) {
      this.takeRecord(drag, event);
      return;
    }
    const width = (event.currentTarget as HTMLElement).getBoundingClientRect().width;
    const at =
      drag.view === 'overview'
        ? this.overviewTime(event)
        : drag.startS - ((event.clientX - drag.startX) / width) * this.zoomSpan();
    drag.shownS = at;
    if (event.timeStamp - drag.sentAt < HAND_EVERY_MS) return;
    drag.sentAt = event.timeStamp;
    this.engine.hand(this.deck(), at, event.timeStamp);
  }

  release(event: PointerEvent) {
    if (this.drag?.pointer === event.pointerId) this.cancel();
  }

  /** The press taken by the system (a scroll, a gesture) or its capture lost. */
  cancelled(event: PointerEvent) {
    if (this.drag?.pointer === event.pointerId) this.cancel();
  }

  /** The hand off the track: play goes on as before. */
  private cancel() {
    const drag = this.drag;
    if (!drag) return;
    this.stopHolding();
    this.drag = null;
    if (drag.held) this.engine.hand(drag.deck, null);
  }

  /** The press has stayed still: once it moves, it takes the record. */
  private holdRecord() {
    this.holding = null;
    const drag = this.drag;
    if (drag?.touch === null) drag.touch = 'hand';
  }

  /** The hand on the record where it is now, moving it from where the press is. */
  private takeRecord(drag: Drag, event: PointerEvent) {
    drag.held = true;
    drag.startS = this.engine.livePosition(this.deck()) ?? drag.startS;
    drag.startX = event.clientX;
    drag.sentAt = event.timeStamp;
    drag.shownS = drag.startS;
    this.engine.hand(this.deck(), drag.startS, event.timeStamp);
  }

  private stopHolding() {
    if (this.holding !== null) clearTimeout(this.holding);
    this.holding = null;
  }

  /** The press slides the edge of the jog, a share of the way it slides: a playing track
   * pushed or held back for a moment, a stopped one moved. */
  private turnEdge(drag: Drag, event: PointerEvent) {
    const width = (event.currentTarget as HTMLElement).getBoundingClientRect().width;
    const seconds = -((event.clientX - drag.lastX) / width) * this.zoomSpan() * EDGE_SHARE;
    drag.lastX = event.clientX;
    if (seconds === 0) return;
    // a stopped track moves by what the edge turns; a playing one is only pushed or held back
    if (!this.engine.status()?.decks[this.deck()]?.playing) {
      const from = drag.shownS ?? this.engine.livePosition(this.deck()) ?? 0;
      drag.shownS = Math.min(Math.max(from + seconds, 0), this.track()?.durationS ?? 0);
    }
    this.engine.jog(this.deck(), seconds, event.timeStamp);
  }

  private overviewTime(event: PointerEvent): number {
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const share = Math.min(Math.max((event.clientX - rect.left) / rect.width, 0), 1);
    return share * (this.track()?.durationS ?? 0);
  }

  /** Seconds of the track shown in the scrolling view: ZOOM_SPAN_S of real time, so that
   * beatmatched decks show their beats at the same spacing. */
  private zoomSpan(): number {
    return ZOOM_SPAN_S * (this.engine.status()?.decks[this.deck()]?.rate ?? 1);
  }

  private draw() {
    const track = this.track();
    const zoom = this.fit(this.zoomCanvas().nativeElement);
    const overview = this.fit(this.overviewCanvas().nativeElement);
    const lanesCanvas = this.lanesCanvas()?.nativeElement;
    const lanes = lanesCanvas ? this.fit(lanesCanvas) : null;
    const activity = this.activity();
    if (!zoom || !overview) {
      // hidden, its canvases emptied: drawn again once shown, however still the deck
      this.showing.forget();
      return;
    }
    if (!track) {
      zoom.ctx.clearRect(0, 0, zoom.size.width, zoom.size.height);
      overview.ctx.clearRect(0, 0, overview.size.width, overview.size.height);
      this.showing.forget();
      return;
    }
    const position = this.drag?.shownS ?? this.engine.livePosition(this.deck()) ?? 0;
    const loop = this.engine.status()?.decks[this.deck()]?.looping ?? null;
    const span = this.zoomSpan();
    const waveform = this.engine.waveforms()[this.deck()];
    this.showing
      .start()
      .with(track)
      .with(waveform)
      .with(position)
      .with(span)
      .with(loop?.active)
      .with(loop?.loopInS)
      .with(loop?.startS)
      .with(loop?.endS)
      .with(zoom.size.width)
      .with(zoom.size.height)
      .with(overview.size.width)
      .with(overview.size.height)
      .with(activity)
      .with(lanes?.size.width)
      .with(lanes?.size.height);
    if (!this.showing.isNew()) return;
    const overlay: WaveformOverlay = { tempo: track.tempo, cues: track.cues, loop };
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
    const { width, height } = fitToDevicePixels(canvas, this.redraw);
    const ctx = canvas.getContext('2d');
    const laidOut = width > 0 && height > 0;
    return ctx && laidOut
      ? { ctx, size: { width, height, ratio: window.devicePixelRatio || 1 } }
      : null;
  }
}
