import { Component, computed, inject, input, signal } from '@angular/core';
import { EngineService } from '../../../core/services/engine.service';
import { MixerService } from '../../../core/services/mixer.service';
import { deckReadouts } from '../../../core/deck-readouts';
import { DECK_MODES, deckVinyl } from '../../../core/deck-vinyl';
import { decimal, deckLetter, formatTime } from '../../../shared/time';
import { camelotHue, camelotMajor, keyName } from '../../../shared/camelot';
import { levelMeter } from '../../../shared/level';
import { Cover } from '../../../shared/cover/cover';
import { GridEditor } from '../grid-editor/grid-editor';
import { TPipe } from '../../../core/i18n/t.pipe';

/** Segments of the meter of what goes to the external mixer. */
const SEND_SEGMENTS = 10;

/**
 * Deck header: letter (lit while playing) with the level sent to an external mixer, cover
 * and title, who drives the deck, and the three readouts seen from afar: remaining time,
 * BPM, key.
 */
@Component({
  selector: 'app-deck-card',
  templateUrl: './deck-card.html',
  styleUrl: './deck-card.scss',
  imports: [Cover, GridEditor, TPipe],
})
export class DeckCard {
  readonly deck = input.required<number>();

  protected readonly engine = inject(EngineService);
  private readonly mixer = inject(MixerService);
  protected readonly letter = computed(() => deckLetter(this.deck()));
  /** The level sent to the deck's outputs of an external mixer (the software mixer has
   * its own meters). */
  private readonly sent = levelMeter(
    () => this.engine.status()?.decks[this.deck()]?.peak ?? 0,
    SEND_SEGMENTS,
  );
  protected readonly send = computed(() => (this.mixer.internal() ? null : this.sent()));
  protected readonly outputs = computed(() => this.mixer.outputsOf(this.deck()));
  private readonly readouts = deckReadouts(this.deck);
  protected readonly track = this.readouts.track;
  protected readonly status = this.readouts.status;
  protected readonly loading = computed(() => this.engine.loading()[this.deck()]);
  /** The grid and key editor is open. */
  protected readonly editing = signal(false);

  private readonly vinyl = deckVinyl(this.deck);
  protected readonly modes = DECK_MODES;
  protected readonly mode = this.vinyl.mode;
  protected readonly signal = this.vinyl.signal;
  protected readonly signalTitle = this.vinyl.signalTitle;
  protected readonly setMode = this.vinyl.setMode;

  protected readonly elapsed = computed(() => formatTime(this.status()?.positionS ?? 0));
  protected readonly length = computed(() => formatTime(this.status()?.durationS ?? 0));
  protected readonly remaining = this.readouts.remaining;
  protected readonly ending = this.readouts.ending;
  protected readonly bpm = this.readouts.bpm;
  /** The track's own tempo and the pitch applied to it. */
  protected readonly tempoDetail = computed(() => {
    const tempo = this.track()?.tempo;
    return tempo ? `${decimal(tempo.bpm, 2)} ${this.readouts.pitch()}` : '';
  });
  protected readonly key = this.readouts.key;
  protected readonly keyShifted = computed(
    () => this.key()?.camelot !== this.track()?.key?.camelot,
  );
  protected readonly hue = camelotHue;
  protected readonly major = camelotMajor;
  protected readonly keyName = keyName;
}
