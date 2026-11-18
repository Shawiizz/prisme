import { Component, computed, inject, input } from '@angular/core';
import { deckReadouts } from '../../core/deck-readouts';
import { EngineService } from '../../core/services/engine.service';
import { fileDropAttribute } from '../../core/services/file-drop.service';
import { DeckDrop } from '../../features/deck/deck-drop/deck-drop';
import { TPipe } from '../../core/i18n/t.pipe';
import { KeyChip } from '../../shared/key-chip/key-chip';
import { deckLetter } from '../../shared/time';

/** A deck as the main screen recalls it while the decks are on the screen below; a track
 * dragged from the file manager onto it is loaded there. */
@Component({
  selector: 'app-now-deck',
  template: `
    <span class="pr-tile" [class.pr-tile--outline]="!status()?.playing">{{ letter() }}</span>
    @if (track(); as t) {
      <span class="names"
        >{{ t.title }}
        @if (t.artist) {
          <span>· {{ t.artist }}</span>
        }
      </span>
    } @else {
      <span class="names empty">{{ 'No track loaded' | t }}</span>
    }
    <span class="bpm pr-num">{{ bpm() ?? '' }}</span>
    @if (key(); as k) {
      <app-key-chip fill [camelot]="k.camelot" />
    }
    <span class="time pr-num" [class]="ending() ?? ''">{{ remaining() }}</span>
    <i class="played" [style.--played]="played()"></i>
    <app-deck-drop [deck]="deck()" />
  `,
  styleUrl: './now-playing.scss',
  host: { '[attr.data-file-drop]': 'fileDrop()' },
  imports: [DeckDrop, KeyChip, TPipe],
})
export class NowDeck {
  readonly deck = input.required<number>();

  private readonly readouts = deckReadouts(this.deck);
  protected readonly letter = computed(() => deckLetter(this.deck()));
  protected readonly track = this.readouts.track;
  protected readonly status = this.readouts.status;
  protected readonly remaining = this.readouts.remaining;
  protected readonly ending = this.readouts.ending;
  protected readonly bpm = this.readouts.bpm;
  protected readonly key = this.readouts.key;
  protected readonly fileDrop = computed(() =>
    fileDropAttribute({ kind: 'deck', deck: this.deck() }),
  );
  /** How much of the track has played, 0 to 1. */
  protected readonly played = computed(() => {
    const s = this.status();
    return s?.loaded && s.durationS > 0 ? Math.min(s.positionS / s.durationS, 1) : 0;
  });
}

/** What plays, recalled on the main screen while the decks are on the screen below. */
@Component({
  selector: 'app-now-playing',
  template: `
    @for (deck of decks(); track deck) {
      <app-now-deck [deck]="deck" />
    }
  `,
  styles: `
    :host {
      display: grid;
      grid-auto-flow: column;
      grid-auto-columns: minmax(0, 1fr);
      border-bottom: 1px solid var(--pr-line);
      background: var(--pr-base);
    }
    app-now-deck + app-now-deck {
      border-left: 1px solid var(--pr-line);
    }
  `,
  imports: [NowDeck],
})
export class NowPlaying {
  protected readonly decks = inject(EngineService).deckIndexes;
}
