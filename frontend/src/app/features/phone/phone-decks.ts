import { Component, booleanAttribute, computed, inject, input, signal } from '@angular/core';
import { landscape } from '../../core/orientation';
import { EngineService } from '../../core/services/engine.service';
import { MixerService } from '../../core/services/mixer.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { Fader } from '../../shared/fader/fader';
import { PhaseStrip } from '../deck/phase-strip/phase-strip';
import { PhoneDeck } from './phone-deck';
import { PhoneDeckPanel } from './phone-deck-panel';
import { PhoneLibrary } from './phone-library';
import { PhoneTouch } from './phone-touch';

/**
 * The decks on a phone. The hands on a controller or on records, they are read from a step
 * away: wide waveforms, the time left, and the next tracks of the list under them. With
 * `touch`, the thumbs play: the decks shrink to a line and their controls come to the
 * bottom, or, the phone held sideways, to each side with the crossfader between them.
 */
@Component({
  selector: 'app-phone-decks',
  template: `
    @if (touch() && wide()) {
      <app-phone-touch side="left" [decks]="lefts()" />
    }
    <section class="decks">
      @for (deck of decks(); track deck) {
        <app-phone-deck
          [deck]="deck"
          [compact]="touch() || wide()"
          (opened)="panel.set({ deck })"
        />
        @if (deck === 0 && !touch()) {
          <app-phase-strip />
        }
      }
      @if (touch() && wide() && mixer.internal()) {
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
    </section>
    @if (!touch()) {
      <app-phone-library peek />
    } @else if (wide()) {
      <app-phone-touch side="right" [decks]="rights()" />
    } @else {
      <app-phone-touch [decks]="decks()" />
    }
    @if (panel(); as open) {
      <app-phone-deck-panel [deck]="open.deck" (closed)="panel.set(null)" />
    }
  `,
  styleUrl: './phone-decks.scss',
  host: { '[class.touch]': 'touch()' },
  imports: [Fader, PhaseStrip, PhoneDeck, PhoneDeckPanel, PhoneLibrary, PhoneTouch, TPipe],
})
export class PhoneDecks {
  /** Nothing is plugged in: the thumbs play. */
  readonly touch = input(false, { transform: booleanAttribute });

  protected readonly mixer = inject(MixerService);
  protected readonly decks = inject(EngineService).deckIndexes;
  protected readonly wide = landscape();
  /** Held sideways, the decks of each side of the crossfader are under the thumb of that side. */
  protected readonly lefts = computed(() => this.decks().filter((d) => d % 2 === 0));
  protected readonly rights = computed(() => this.decks().filter((d) => d % 2 === 1));
  /** The deck whose panel is open. */
  protected readonly panel = signal<{ deck: number } | null>(null);
}
