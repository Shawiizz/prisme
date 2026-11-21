import { Component, booleanAttribute, inject, input, output, viewChild } from '@angular/core';
import { FxService } from '../../core/services/fx.service';
import { DeckPads } from '../deck/deck-pads/deck-pads';
import { PhoneFloatHead } from './phone-float-head';
import { inSight } from './phone-sight';

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
  private readonly pads = viewChild(DeckPads);

  constructor() {
    const fx = inject(FxService);
    inSight(() => ({
      kind: 'pads',
      deck: this.deck(),
      mode: this.pads()?.mode() ?? 'cues',
      fxSlot: fx.chosen(this.deck()),
    }));
  }
}
