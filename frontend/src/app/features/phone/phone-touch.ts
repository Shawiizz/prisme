import { Component, inject, input, linkedSignal } from '@angular/core';
import { MixerService } from '../../core/services/mixer.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { Fader } from '../../shared/fader/fader';
import { deckLetter } from '../../shared/time';
import { DeckControls } from '../deck/deck-controls/deck-controls';
import { DeckPads } from '../deck/deck-pads/deck-pads';

/**
 * A deck under the thumbs, at the bottom of a phone nothing is plugged into: which deck,
 * its pads, the crossfader, and CUE, PLAY and SYNC lowest, where the thumbs rest. Held
 * sideways, a deck has a `side` of the screen to itself and the crossfader is between them.
 */
@Component({
  selector: 'app-phone-touch',
  template: `
    @if (decks().length > 1) {
      <div class="which" role="radiogroup" [attr.aria-label]="'Deck' | t">
        @for (d of decks(); track d) {
          <button role="radio" [attr.aria-checked]="deck() === d" (click)="deck.set(d)">
            {{ letter(d) }}
          </button>
        }
      </div>
    }
    <app-deck-pads [deck]="deck()" />
    @if (!side() && mixer.internal()) {
      <div class="cross">
        <span>A</span>
        <app-fader
          touch
          centered
          [min]="-1"
          [max]="1"
          [value]="mixer.crossfader()"
          (valueChange)="mixer.setCrossfader($event)"
          [label]="'Crossfader' | t"
        />
        <span>B</span>
      </div>
    }
    <app-deck-controls [deck]="deck()" />
  `,
  styleUrl: './phone-touch.scss',
  host: { '[class]': 'side()' },
  imports: [DeckControls, DeckPads, Fader, TPipe],
})
export class PhoneTouch {
  /** The decks these controls can be given to. */
  readonly decks = input.required<number[]>();
  /** Held sideways: the side of the screen these controls are on. */
  readonly side = input<'left' | 'right' | ''>('');

  protected readonly mixer = inject(MixerService);
  /** The deck under the thumbs: the first, until another is chosen (and while it is there). */
  protected readonly deck = linkedSignal<number[], number>({
    source: this.decks,
    computation: (decks, previous) =>
      previous && decks.includes(previous.value) ? previous.value : decks[0],
  });
  protected readonly letter = deckLetter;
}
