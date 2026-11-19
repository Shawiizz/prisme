import { Component, booleanAttribute, computed, inject, input, signal } from '@angular/core';
import { landscape, short } from '../../core/orientation';
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
      <app-phone-table animate.enter="pr-fade-in" (opened)="panel.set({ deck: $event })" />
    } @else {
      <section class="decks" [class.four]="four()" animate.enter="pr-fade-in">
        @for (deck of decks(); track deck) {
          <app-phone-deck
            [deck]="deck"
            [compact]="low() || four()"
            (opened)="panel.set({ deck })"
          />
          @if (deck === 0) {
            <app-phase-strip />
          }
        }
      </section>
      @if (!four() || wide()) {
        <app-phone-library peek animate.enter="pr-fade-in" />
      }
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
  /** Four decks take an upright screen: a line each over its waveform, the library a tab
   * away. */
  protected readonly four = computed(() => this.decks().length > 2);
  /** The screen leaves each deck a line over its waveform. */
  protected readonly low = short();
  /** Sideways, the library is beside the decks, whatever their number. */
  protected readonly wide = landscape();
  /** The deck whose panel is open. */
  protected readonly panel = signal<{ deck: number } | null>(null);
}
