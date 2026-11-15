import { Component, computed, inject, input, output, signal } from '@angular/core';
import { closedByBack } from '../../core/back';
import { deckTempo } from '../../core/deck-tempo';
import { DeckMode, deckVinyl } from '../../core/deck-vinyl';
import { BANDS, Band } from '../../core/models/engine';
import { EngineService } from '../../core/services/engine.service';
import { MixerService } from '../../core/services/mixer.service';
import { t } from '../../core/i18n/i18n';
import { TPipe } from '../../core/i18n/t.pipe';
import { CueButton } from '../../shared/cue-button/cue-button';
import { Fader } from '../../shared/fader/fader';
import { eqGain, eqKnob } from '../../shared/mixer-scales';
import { deckLetter } from '../../shared/time';
import { GridEditor } from '../deck/grid-editor/grid-editor';
import { PhoneTempo } from './phone-tempo';

const DRIVERS: readonly { id: DeckMode; label: string }[] = [
  {
    id: 'int',
    get label() {
      return t('Itself');
    },
  },
  {
    id: 'relative',
    get label() {
      return t('Vinyl REL');
    },
  },
  {
    id: 'absolute',
    get label() {
      return t('Vinyl ABS');
    },
  },
];
const EQ: readonly { band: Band; label: string }[] = [
  { band: 'low', label: 'LOW' },
  { band: 'mid', label: 'MID' },
  { band: 'high', label: 'HI' },
];

/**
 * What is set on a deck now and then, risen over the screen: who drives it, its pitch, its
 * channel of the software mixer as wide faders, its key lock, its grid and its headphones.
 */
@Component({
  selector: 'app-phone-deck-panel',
  templateUrl: './phone-deck-panel.html',
  styleUrl: './phone-deck-panel.scss',
  imports: [CueButton, Fader, GridEditor, PhoneTempo, TPipe],
})
export class PhoneDeckPanel {
  readonly deck = input.required<number>();
  /** Opened from the table, which has the channel already. */
  readonly table = input(false);
  readonly closed = output<void>();

  protected readonly engine = inject(EngineService);
  protected readonly mixer = inject(MixerService);
  private readonly vinyl = deckVinyl(this.deck);
  protected readonly tempo = deckTempo(this.deck);

  protected readonly letter = computed(() => deckLetter(this.deck()));
  protected readonly track = computed(() => this.engine.tracks()[this.deck()]);
  protected readonly settings = computed(() => this.engine.decks()[this.deck()]);
  protected readonly drivers = DRIVERS;
  protected readonly mode = this.vinyl.mode;
  protected readonly setMode = this.vinyl.setMode;
  /** What the record says, while it drives the deck. */
  protected readonly signalTitle = computed(() =>
    this.vinyl.signal() === null ? null : this.vinyl.signalTitle(),
  );

  protected readonly eq = EQ;
  /** The grid and key editor is open. */
  protected readonly editing = signal(false);

  constructor() {
    closedByBack(this.editing, () => this.editing.set(false));
  }

  protected eqKnob(band: Band): number {
    return eqKnob(this.mixer.strip(this.deck()).eq[BANDS.indexOf(band)]);
  }

  protected setEq(band: Band, knob: number) {
    this.mixer.setEq(this.deck(), band, eqGain(knob));
  }

  protected filterKnob(): number {
    return (this.mixer.strip(this.deck()).filter + 1) / 2;
  }
}
