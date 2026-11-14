import { Component, computed, inject, input } from '@angular/core';
import { deckReadouts } from '../../core/deck-readouts';
import { EngineService } from '../../core/services/engine.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { camelotHue, camelotMajor } from '../../shared/camelot';
import { deckLetter } from '../../shared/time';

/** A deck as the main screen recalls it while the decks are on the screen below. */
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
      <span
        class="pr-key pr-key--fill"
        [class.pr-key--maj]="major(k.camelot)"
        [style.--h]="hue(k.camelot)"
        >{{ k.camelot }}</span
      >
    }
    <span class="time pr-num" [class]="ending() ?? ''">{{ remaining() }}</span>
    <i class="played" [style.--played]="played()"></i>
  `,
  styleUrl: './now-playing.scss',
  imports: [TPipe],
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
  /** How much of the track has played, 0 to 1. */
  protected readonly played = computed(() => {
    const s = this.status();
    return s?.loaded && s.durationS > 0 ? Math.min(s.positionS / s.durationS, 1) : 0;
  });
  protected readonly hue = camelotHue;
  protected readonly major = camelotMajor;
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
