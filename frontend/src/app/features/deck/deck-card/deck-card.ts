import {
  Component,
  booleanAttribute,
  computed,
  inject,
  input,
  signal,
  viewChild,
} from '@angular/core';
import { EngineService } from '../../../core/services/engine.service';
import { MixerService } from '../../../core/services/mixer.service';
import { deckReadouts } from '../../../core/deck-readouts';
import { DECK_MODES, deckVinyl } from '../../../core/deck-vinyl';
import { decimal, deckLetter, formatTime } from '../../../shared/time';
import { musicalKeyName } from '../../../shared/musical-key';
import { KeyChip } from '../../../shared/key-chip/key-chip';
import { sameSegments, segments } from '../../../shared/meter';
import { Cover } from '../../../shared/cover/cover';
import { DeckMenu } from '../deck-menu/deck-menu';
import { GridEditor } from '../grid-editor/grid-editor';
import { TPipe } from '../../../core/i18n/t.pipe';

/** Segments of the meter of what goes to the external mixer. */
const SEND_SEGMENTS = 10;

/**
 * Deck header: letter (lit while playing) with the level sent to an external mixer, cover
 * and title, who drives the deck, its menu, and the three readouts seen from afar: remaining
 * time, BPM, key. A right click on it opens its menu there.
 */
@Component({
  selector: 'app-deck-card',
  templateUrl: './deck-card.html',
  styleUrl: './deck-card.scss',
  host: {
    '[class.compact]': 'compact()',
    '[class.band]': 'band()',
    '[class.tall]': 'tall()',
    '(contextmenu)': 'menu().openAt($event)',
  },
  imports: [Cover, DeckMenu, GridEditor, KeyChip, TPipe],
})
export class DeckCard {
  readonly deck = input.required<number>();
  /** Half the height, for four decks. */
  readonly compact = input(false, { transform: booleanAttribute });
  /** On one line across the decks' own window, over or under its waveform: the figures large,
   * read from afar. */
  readonly band = input(false, { transform: booleanAttribute });
  /** On a tall lane of the decks' own window: the track and the figures larger, the time over
   * the tempo and key. */
  readonly tall = input(false, { transform: booleanAttribute });

  protected readonly engine = inject(EngineService);
  protected readonly menu = viewChild.required(DeckMenu);
  private readonly mixer = inject(MixerService);
  protected readonly letter = computed(() => deckLetter(this.deck()));
  /** The level sent to the deck's outputs of an external mixer (the software mixer has
   * its own meters). */
  protected readonly send = computed(
    () =>
      this.mixer.internal()
        ? null
        : segments(this.engine.status()?.meters.decks[this.deck()], SEND_SEGMENTS),
    { equal: sameSegments },
  );
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
  protected readonly vinylOffered = this.vinyl.offered;
  protected readonly signal = this.vinyl.signal;
  protected readonly signalTitle = this.vinyl.signalTitle;
  protected readonly setMode = this.vinyl.setMode;
  /** What the way the deck is driven does, said over its narrow key. */
  protected readonly modeTitle = computed(
    () => DECK_MODES.find((m) => m.id === this.mode())?.title() ?? '',
  );

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
  protected readonly keyName = musicalKeyName;
}
