import { Component, computed, inject, input, output, signal } from '@angular/core';
import { GridChange } from '../../../core/models/engine';
import { EngineService } from '../../../core/services/engine.service';
import { TrackEditService } from '../../../core/services/track-edit.service';
import { TapTempo } from '../../../shared/tap-tempo';
import { TPipe } from '../../../core/i18n/t.pipe';
import { camelotHue, camelotMajor, keyName } from '../../../shared/camelot';
import { Dismiss, overlay } from '../../../shared/overlay';
import { PHONE } from '../../../core/platform';
import { phaseOffset } from '../../../core/sync';

/** Grid moves of the arrow buttons: fine and coarse (seconds). */
const FINE_SHIFT_S = 0.002;
const COARSE_SHIFT_S = 0.02;

/** Corrections of the beat grid and key of the track on a deck, and its key shift. */
@Component({
  selector: 'app-grid-editor',
  templateUrl: './grid-editor.html',
  styleUrl: './grid-editor.scss',
  imports: [TPipe, Dismiss],
})
export class GridEditor {
  /** On a phone it rises from the bottom, over the panel of its deck. */
  protected readonly PHONE = PHONE;
  protected readonly hue = camelotHue;
  protected readonly major = camelotMajor;
  protected readonly keyName = keyName;
  readonly deck = input.required<number>();
  readonly closed = output();

  private readonly engine = inject(EngineService);
  private readonly edits = inject(TrackEditService);
  private readonly tapTempo = new TapTempo();

  protected readonly fine = FINE_SHIFT_S;
  protected readonly coarse = COARSE_SHIFT_S;
  protected readonly track = computed(() => this.engine.tracks()[this.deck()]);
  /** What the BPM field shows: the tempo being tapped, else the track's. */
  protected readonly tapped = signal<number | null>(null);
  protected readonly bpmText = computed(() => {
    const bpm = this.tapped() ?? this.track()?.tempo?.bpm;
    return bpm === undefined ? '' : bpm.toFixed(2);
  });
  protected readonly minorKeys = computed(() =>
    this.edits.keys().filter((k) => k.camelot.endsWith('A')),
  );
  protected readonly majorKeys = computed(() =>
    this.edits.keys().filter((k) => k.camelot.endsWith('B')),
  );

  protected readonly keyShift = computed(() => this.track()?.keyShift ?? 0);
  protected readonly shiftLabel = computed(() => {
    const s = this.keyShift();
    return s > 0 ? `+${s}` : s < 0 ? `−${-s}` : '0';
  });
  protected readonly keyMatch = computed(() => this.engine.keyMatch(this.deck()));
  /** How far to move the grid (seconds of the track) so that its beats fall on the other
   * deck's, both playing: matched by ear, the deck keeps what is heard and is in phase. */
  protected readonly toOther = computed(() => {
    const decks = this.engine.status()?.decks ?? [];
    const own = decks[this.deck()];
    const other = decks[this.engine.mixedWith(this.deck())];
    const bpm = this.track()?.tempo?.bpm;
    if (!own?.playing || !other?.playing || !bpm) return null;
    const beats = phaseOffset(own, other);
    return beats === null ? null : (beats * 60) / bpm;
  });

  constructor() {
    this.edits.loadKeys();
    // the deck that opens it answers the back gesture
    overlay(
      () => true,
      () => this.closed.emit(),
      { back: false },
    );
  }

  grid(change: GridChange) {
    this.tapped.set(null);
    this.edits.editGrid(this.deck(), change);
  }

  alignToOther() {
    const seconds = this.toOther();
    if (seconds !== null) this.grid({ kind: 'shift', seconds });
  }

  setBpm(text: string) {
    const bpm = Number.parseFloat(text.replace(',', '.'));
    if (Number.isFinite(bpm)) this.grid({ kind: 'bpm', bpm });
  }

  tap() {
    const bpm = this.tapTempo.tap(performance.now());
    if (bpm !== null) this.tapped.set(bpm);
  }

  shiftKey(semitones: number) {
    this.engine.setKeyShift(this.deck(), semitones);
  }

  setKey(camelot: string | null) {
    this.edits.setKey(this.deck(), camelot);
  }
}
