import { Component, booleanAttribute, inject, input, signal } from '@angular/core';
import { closedByBack } from '../../core/back';
import { landscape } from '../../core/orientation';
import { EngineService } from '../../core/services/engine.service';
import { PhaseStrip } from '../deck/phase-strip/phase-strip';
import { PhoneDeck } from './phone-deck';
import { PhoneDeckPanel } from './phone-deck-panel';
import { PhoneLibrary } from './phone-library';
import { PhoneTable } from './phone-table';

/**
 * The decks on a phone. The hands on a controller or on records, they are read from a step
 * away: wide waveforms, the time left, and the next tracks of the list under them. With
 * `touch`, the phone is played as a table.
 */
@Component({
  selector: 'app-phone-decks',
  template: `
    @if (touch()) {
      <app-phone-table (opened)="panel.set({ deck: $event })" />
    } @else {
      <section class="decks">
        @for (deck of decks(); track deck) {
          <app-phone-deck [deck]="deck" [compact]="wide()" (opened)="panel.set({ deck })" />
          @if (deck === 0) {
            <app-phase-strip />
          }
        }
      </section>
      <app-phone-library peek />
    }
    @if (panel(); as open) {
      <app-phone-deck-panel [deck]="open.deck" [table]="touch()" (closed)="panel.set(null)" />
    }
  `,
  styleUrl: './phone-decks.scss',
  imports: [PhaseStrip, PhoneDeck, PhoneDeckPanel, PhoneLibrary, PhoneTable],
})
export class PhoneDecks {
  /** Nothing is plugged in: the thumbs play. */
  readonly touch = input(false, { transform: booleanAttribute });

  protected readonly decks = inject(EngineService).deckIndexes;
  protected readonly wide = landscape();
  /** The deck whose panel is open. */
  protected readonly panel = signal<{ deck: number } | null>(null);

  constructor() {
    closedByBack(
      () => this.panel() !== null,
      () => this.panel.set(null),
    );
  }
}
