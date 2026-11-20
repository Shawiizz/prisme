import { Component, inject, input } from '@angular/core';
import { deckTempo } from '../../core/deck-tempo';
import { EngineService } from '../../core/services/engine.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { Fader } from '../../shared/fader/fader';

/**
 * The tempo fader of a deck on a phone, across the screen between the ends of its range. A
 * record or sync holding the tempo, it shows the rate played.
 */
@Component({
  selector: 'app-phone-tempo',
  template: `
    <span class="pr-num">−{{ tempo.rangePercent() }}</span>
    <app-fader
      touch
      centered
      [value]="tempo.position()"
      [defaultValue]="0.5"
      (valueChange)="engine.moveTempo(deck(), $event)"
      [disabled]="tempo.held()"
      [label]="'Tempo' | t"
    />
    <span class="pr-num">+{{ tempo.rangePercent() }}</span>
  `,
  styles: `
    :host {
      display: flex;
      align-items: center;
      gap: 6px;
      font-size: var(--pr-size-small);
      color: var(--pr-text-3);
    }
    app-fader {
      flex: 1;
    }
  `,
  imports: [Fader, TPipe],
})
export class PhoneTempo {
  readonly deck = input.required<number>();

  protected readonly engine = inject(EngineService);
  protected readonly tempo = deckTempo(this.deck);
}
