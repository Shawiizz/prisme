import { Component, computed, inject, input, signal } from '@angular/core';
import { PerformanceService } from '../../../core/services/performance.service';
import { EngineService } from '../../../core/services/engine.service';
import { TPipe } from '../../../core/i18n/t.pipe';
import { Dismiss, overlay } from '../../../shared/overlay';
import { DeckOptions } from '../deck-options/deck-options';

/** What a deck does now and then, off the deck until asked for (see `DeckOptions`); what of
 * it is on and changes what is heard shows beside it, a press away from off. */
@Component({
  selector: 'app-deck-menu',
  template: `
    @if (reversed()) {
      <button
        class="pr-btn pr-btn--sm is-on"
        [title]="'The track plays backwards: press to play it forwards' | t"
        (click)="performance.setReverse(deck(), false)"
      >
        REV
      </button>
    }
    @if (muted()) {
      <button
        class="pr-btn pr-btn--sm is-on"
        [title]="'The channel is muted: press to hear it again' | t"
        (click)="engine.setMute(deck(), false)"
      >
        MUTE
      </button>
    }
    <button
      class="pr-btn pr-btn--sm pr-btn--icon"
      [class.is-on]="open()"
      (click)="open.set(!open())"
      [title]="'Deck options: motor, reverse, mute, repeat, tempo range, fader start' | t"
      [attr.aria-label]="'Deck options: motor, reverse, mute, repeat, tempo range, fader start' | t"
    >
      ⋯
    </button>
    @if (open()) {
      <div class="pr-veil" (dismiss)="open.set(false)"></div>
      <app-deck-options class="menu pr-popover" [deck]="deck()" />
    }
  `,
  styles: `
    :host {
      position: relative;
      flex: none;
      display: flex;
      align-items: center;
      gap: var(--pr-space-1);
    }
    .pr-btn:not(.pr-btn--icon) {
      padding: 0 7px;
      font: 600 10.5px / 1 var(--pr-font);
      letter-spacing: 0.04em;
    }
    .menu {
      position: absolute;
      top: calc(100% + 6px);
      right: 0;
      width: 264px;
      padding: var(--pr-space-1) var(--pr-space-3);
      box-shadow: 0 16px 40px rgb(0 0 0 / 0.5);
    }
  `,
  imports: [DeckOptions, Dismiss, TPipe],
})
export class DeckMenu {
  readonly deck = input.required<number>();
  protected readonly open = signal(false);
  protected readonly performance = inject(PerformanceService);
  protected readonly engine = inject(EngineService);
  protected readonly reversed = computed(
    () => this.engine.status()?.decks[this.deck()]?.reverse ?? false,
  );
  protected readonly muted = computed(() => this.engine.decks()[this.deck()].mute);

  constructor() {
    overlay(this.open, () => this.open.set(false));
  }
}
