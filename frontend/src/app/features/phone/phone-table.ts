import { NgTemplateOutlet } from '@angular/common';
import { Component, computed, inject, linkedSignal, output, signal } from '@angular/core';
import { closedByBack } from '../../core/back';
import { landscape } from '../../core/orientation';
import { EngineService } from '../../core/services/engine.service';
import { MixerService } from '../../core/services/mixer.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { Fader } from '../../shared/fader/fader';
import { deckLetter } from '../../shared/time';
import { DeckControls } from '../deck/deck-controls/deck-controls';
import { PhoneChannel } from './phone-channel';
import { PhoneDeck } from './phone-deck';
import { PhonePads } from './phone-pads';
import { PhonePitch } from './phone-pitch';

type Side = 'a' | 'b';
type Floating = { kind: 'pads' | 'pitch'; side: Side };

/** The deck of a side: its first, until another of that side is chosen (while it is there). */
function sideDeck(decks: () => number[]) {
  return linkedSignal<number[], number>({
    source: decks,
    computation: (all, previous) =>
      previous && all.includes(previous.value) ? previous.value : all[0],
  });
}

/**
 * A phone nothing is plugged into, played as a table: the waveforms of both sides, a
 * channel each, the crossfader, and under each thumb the keys of its deck. Its pads and its
 * pitch float over the table when asked for. Held sideways, the pads and the keys of each
 * deck go down its side of the screen.
 */
@Component({
  selector: 'app-phone-table',
  templateUrl: './phone-table.html',
  styleUrl: './phone-table.scss',
  imports: [
    DeckControls,
    Fader,
    NgTemplateOutlet,
    PhoneChannel,
    PhoneDeck,
    PhonePads,
    PhonePitch,
    TPipe,
  ],
})
export class PhoneTable {
  /** The settings of a deck are asked for. */
  readonly opened = output<number>();

  protected readonly mixer = inject(MixerService);
  private readonly decks = inject(EngineService).deckIndexes;
  protected readonly wide = landscape();
  /** The decks on each side of the crossfader: A and C on the left, B and D on the right. */
  protected readonly lefts = computed(() => this.decks().filter((d) => d % 2 === 0));
  protected readonly rights = computed(() => this.decks().filter((d) => d % 2 === 1));
  protected readonly left = sideDeck(this.lefts);
  protected readonly right = sideDeck(this.rights);
  protected readonly letter = deckLetter;
  /** What floats over the table. */
  protected readonly floating = signal<Floating | null>(null);
  protected readonly floatingDeck = computed(() => {
    const floating = this.floating();
    return floating && this.deckOf(floating.side);
  });

  constructor() {
    closedByBack(
      () => this.floating() !== null,
      () => this.floating.set(null),
    );
  }

  protected deckOf(side: Side): number {
    return side === 'a' ? this.left() : this.right();
  }

  protected decksOf(side: Side): number[] {
    return side === 'a' ? this.lefts() : this.rights();
  }

  protected choose(side: Side, deck: number) {
    (side === 'a' ? this.left : this.right).set(deck);
  }

  /** Opens what floats, or closes it when it is the one open. */
  protected toggle(kind: Floating['kind'], side: Side) {
    const open = this.floating();
    this.floating.set(open?.kind === kind && open.side === side ? null : { kind, side });
  }

  protected isOpen(kind: Floating['kind'], side: Side): boolean {
    const open = this.floating();
    return open?.kind === kind && open.side === side;
  }
}
