import { Component, computed, inject, input, output, viewChild } from '@angular/core';
import { DeckStatus, GridChange } from '../../../core/models/engine';
import { EngineService } from '../../../core/services/engine.service';
import { TrackEditService } from '../../../core/services/track-edit.service';
import { TPipe } from '../../../core/i18n/t.pipe';
import { camelotHue, camelotMajor, keyName } from '../../../shared/camelot';
import { Dismiss, overlay } from '../../../shared/overlay';
import { PHONE } from '../../../core/platform';
import { Sheet } from '../../../shared/sheet';

/** The coarse arrows move the grid by so many of the backend's steps (a step: 5 ms), the fine
 * ones by one, as a controller's grid keys do. */
const COARSE_STEPS = 4;

/** Corrections of the beat grid and key of the track on a deck, and its key shift. */
@Component({
  selector: 'app-grid-editor',
  templateUrl: './grid-editor.html',
  styleUrl: './grid-editor.scss',
  imports: [TPipe, Dismiss, Sheet],
})
export class GridEditor {
  /** On a phone it rises from the bottom, over the panel of its deck. */
  protected readonly PHONE = PHONE;
  protected readonly hue = camelotHue;
  protected readonly major = camelotMajor;
  protected readonly keyName = keyName;
  readonly deck = input.required<number>();
  readonly closed = output();
  private readonly sheet = viewChild.required(Sheet);

  private readonly engine = inject(EngineService);
  private readonly edits = inject(TrackEditService);

  protected readonly coarse = COARSE_STEPS;
  protected readonly track = computed(() => this.engine.tracks()[this.deck()]);
  protected readonly bpmText = computed(() => {
    const bpm = this.track()?.tempo?.bpm;
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
  /** Both keys known: the deck's can be matched to the one heard beside it (`key.sync`). */
  protected readonly keysKnown = computed(
    () => !!this.track()?.key && this.engine.heardKey(this.engine.mixedWith(this.deck())) !== null,
  );
  /** Both decks playing on a grid: the grid can move onto the other deck's beats. */
  protected readonly alignable = computed(() => {
    const decks = this.engine.status()?.decks ?? [];
    const own = decks[this.deck()];
    const other = decks[this.engine.mixedWith(this.deck())];
    const onBeats = (d: DeckStatus | undefined) => !!d?.playing && d.beatPhase !== null;
    return !!this.track()?.tempo && onBeats(own) && onBeats(other);
  });

  constructor() {
    this.edits.loadKeys();
    // the deck that opens it answers the back gesture
    overlay(
      () => true,
      () => this.sheet().close(),
      { back: false },
    );
  }

  grid(change: GridChange) {
    this.edits.editGrid(this.deck(), change);
  }

  alignToOther() {
    this.grid({ kind: 'align' });
  }

  setBpm(text: string) {
    const bpm = Number.parseFloat(text.replace(',', '.'));
    if (Number.isFinite(bpm)) this.grid({ kind: 'bpm', bpm });
  }

  /** A key of the deck's (`key.up`, `grid.later`…), as a controller's does it. */
  press(action: string) {
    this.engine.press(this.deck(), action);
  }

  /** The grid moved later (earlier below 0) by `steps` steps, as a controller's encoder. */
  moveGrid(steps: number) {
    this.engine.perform(`deck${this.deck() + 1}.grid.move`, steps, true);
  }

  setKey(camelot: string | null) {
    this.edits.setKey(this.deck(), camelot);
  }
}
