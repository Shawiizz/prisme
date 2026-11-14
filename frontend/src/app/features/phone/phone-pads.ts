import { Component, booleanAttribute, computed, inject, input, output } from '@angular/core';
import { EngineService } from '../../core/services/engine.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { deckLetter } from '../../shared/time';
import { DeckPads } from '../deck/deck-pads/deck-pads';

/**
 * The pads of a deck on a phone: floating over its table, or, held sideways, down the
 * `side` of the screen under the thumb of that deck.
 */
@Component({
  selector: 'app-phone-pads',
  template: `
    @if (!side()) {
      <div class="head">
        <span class="pr-tile" [class.pr-tile--outline]="!playing()">{{ letter() }}</span>
        <b>Pads</b>
        <button
          class="pr-btn pr-btn--sm pr-btn--icon pr-btn--ghost"
          (click)="closed.emit()"
          [attr.aria-label]="'Close' | t"
        >
          ×
        </button>
      </div>
    }
    <app-deck-pads [deck]="deck()" />
  `,
  styleUrl: './phone-float.scss',
  host: { '[class.side]': 'side()' },
  imports: [DeckPads, TPipe],
})
export class PhonePads {
  readonly deck = input.required<number>();
  readonly side = input(false, { transform: booleanAttribute });
  readonly closed = output<void>();

  private readonly engine = inject(EngineService);
  protected readonly letter = computed(() => deckLetter(this.deck()));
  protected readonly playing = computed(
    () => this.engine.status()?.decks[this.deck()]?.playing ?? false,
  );
}
