import { Component, computed, inject, input, output, signal, viewChild } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { EngineService } from '../../../core/services/engine.service';
import { TrackEditService } from '../../../core/services/track-edit.service';
import { TPipe } from '../../../core/i18n/t.pipe';
import { keyHue, musicalKeyName } from '../../../shared/musical-key';
import { Dismiss, overlay } from '../../../shared/overlay';
import { PHONE } from '../../../core/platform';
import { Sheet } from '../../../shared/sheet';
import type { GridChange } from '../../../core/models/backend';

/** How far the fine and the coarse arrows move the grid (s): finer than a controller's steps. */
const FINE_SHIFT_S = 0.002;
const COARSE_SHIFT_S = 0.02;

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
  protected readonly hue = keyHue;
  protected readonly keyName = musicalKeyName;
  readonly deck = input.required<number>();
  readonly closed = output();
  private readonly sheet = viewChild.required(Sheet);

  private readonly engine = inject(EngineService);
  private readonly edits = inject(TrackEditService);

  protected readonly fine = FINE_SHIFT_S;
  protected readonly coarse = COARSE_SHIFT_S;
  protected readonly track = computed(() => this.engine.tracks()[this.deck()]);
  /** What the BPM field shows: the tempo being tapped, for the DJ to apply, else the track's. */
  private readonly tapped = signal<number | null>(null);
  protected readonly bpmText = computed(() => {
    const bpm = this.tapped() ?? this.track()?.tempo?.bpm;
    return bpm === undefined ? '' : bpm.toFixed(2);
  });
  /** The pins of a grid whose tempo changes. */
  protected readonly pins = computed(() => this.track()?.tempo?.anchors.length ?? 0);
  protected readonly minorKeys = computed(() => this.edits.keys().filter((k) => k.minor));
  protected readonly majorKeys = computed(() => this.edits.keys().filter((k) => !k.minor));

  protected readonly keyShift = computed(() => this.track()?.keyShift ?? 0);
  protected readonly shiftLabel = computed(() => {
    const s = this.keyShift();
    return s > 0 ? `+${s}` : s < 0 ? `−${-s}` : '0';
  });
  private readonly status = computed(() => this.engine.deckStatus(this.deck()));
  /** Both keys known: the deck's can be matched to the one heard beside it (`key.sync`). */
  protected readonly keysKnown = computed(() => this.status()?.canSyncKey ?? false);
  /** Both decks playing on a grid: the grid can move onto the other deck's beats. */
  protected readonly alignable = computed(() => this.status()?.canAlignGrid ?? false);

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
    this.tapped.set(null);
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

  /** The grid moved later (earlier below 0) by `seconds`. */
  shift(seconds: number) {
    this.grid({ kind: 'shift', seconds });
  }

  /** A tap of the beat: the tempo tapped shown in the field once there are enough taps. */
  async tap() {
    const bpm = await invoke<number | null>('tap_tempo', { deck: this.deck() });
    if (bpm !== null) this.tapped.set(bpm);
  }

  setKey(camelot: string | null) {
    this.edits.setKey(this.deck(), camelot);
  }
}
