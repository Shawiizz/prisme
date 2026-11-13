import { Component, inject, linkedSignal } from '@angular/core';
import { EngineService } from '../../core/services/engine.service';
import { MixerService } from '../../core/services/mixer.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { deckLetter } from '../../shared/time';
import { DeckControls } from '../deck/deck-controls/deck-controls';
import { DeckPads } from '../deck/deck-pads/deck-pads';
import { ChannelSlot } from '../mixer/channel-slot/channel-slot';
import { Crossfader } from '../mixer/crossfader/crossfader';
import { PhoneDeck } from './phone-deck';

/**
 * A deck under the fingers, for a phone without a controller: the deck chosen at the top,
 * what plays it, its pads, and its channel and the crossfader of the software mixer.
 */
@Component({
  selector: 'app-phone-controls',
  template: `
    <div class="pr-seg pr-seg--lg decks" role="radiogroup" [attr.aria-label]="'Deck' | t">
      @for (d of decks(); track d) {
        <button role="radio" [attr.aria-checked]="deck() === d" (click)="deck.set(d)">
          {{ letter(d) }}
        </button>
      }
    </div>
    <app-phone-deck [deck]="deck()" />
    <div class="touch">
      <app-deck-controls [deck]="deck()" />
      <app-deck-pads [deck]="deck()" />
      @if (mixer.internal()) {
        <app-channel-slot [deck]="deck()" />
        <app-crossfader />
      }
    </div>
  `,
  styleUrl: './phone-controls.scss',
  imports: [ChannelSlot, Crossfader, DeckControls, DeckPads, PhoneDeck, TPipe],
})
export class PhoneControls {
  protected readonly decks = inject(EngineService).deckIndexes;
  protected readonly mixer = inject(MixerService);
  /** The deck under the fingers: the first, until another is chosen (and while it is there). */
  protected readonly deck = linkedSignal<number[], number>({
    source: this.decks,
    computation: (decks, previous) =>
      previous && decks.includes(previous.value) ? previous.value : decks[0],
  });
  protected readonly letter = deckLetter;
}
