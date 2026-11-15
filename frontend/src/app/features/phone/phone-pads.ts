import { Component, booleanAttribute, input, output } from '@angular/core';
import { DeckPads } from '../deck/deck-pads/deck-pads';
import { PhoneFloatHead } from './phone-float-head';

/**
 * The pads of a deck on a phone: floating over its table, or, held sideways, down the
 * `side` of the screen under the thumb of that deck.
 */
@Component({
  selector: 'app-phone-pads',
  template: `
    @if (!side()) {
      <app-phone-float-head [deck]="deck()" name="Pads" (closed)="closed.emit()" />
    }
    <app-deck-pads [deck]="deck()" />
  `,
  styleUrl: './phone-float.scss',
  host: { '[class.side]': 'side()' },
  imports: [DeckPads, PhoneFloatHead],
})
export class PhonePads {
  readonly deck = input.required<number>();
  readonly side = input(false, { transform: booleanAttribute });
  readonly closed = output<void>();
}
