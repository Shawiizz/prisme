import { Component, input, signal } from '@angular/core';
import { TPipe } from '../../../core/i18n/t.pipe';
import { Dismiss, overlay } from '../../../shared/overlay';
import { DeckOptions } from '../deck-options/deck-options';

/** What a deck does now and then, off the deck until asked for (see `DeckOptions`). */
@Component({
  selector: 'app-deck-menu',
  template: `
    <button
      class="pr-btn pr-btn--sm pr-btn--icon"
      [class.is-on]="open()"
      (click)="open.set(!open())"
      [title]="'Deck options: motor, repeat' | t"
      [attr.aria-label]="'Deck options: motor, repeat' | t"
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

  constructor() {
    overlay(this.open, () => this.open.set(false));
  }
}
