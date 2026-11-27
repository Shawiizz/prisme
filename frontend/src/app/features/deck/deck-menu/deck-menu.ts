import { Component, ElementRef, computed, inject, input, signal } from '@angular/core';
import { PerformanceService } from '../../../core/services/performance.service';
import { EngineService } from '../../../core/services/engine.service';
import { TPipe } from '../../../core/i18n/t.pipe';
import { Dismiss, overlay } from '../../../shared/overlay';
import { Popover, popoverAt } from '../../../shared/popover';
import { DeckOptions } from '../deck-options/deck-options';
import { AssignTarget, deckAction } from '../../../shared/assign/assign';

/** Between the menu and its key (CSS px). */
const MENU_GAP_PX = 6;

/** What a deck does now and then, off the deck until asked for (see `DeckOptions`); what of
 * it is on and changes what is heard shows beside it, a press away from off. The menu opens
 * under its key, or where the deck is right-clicked. */
@Component({
  selector: 'app-deck-menu',
  template: `
    @if (reversed()) {
      <button
        class="pr-btn pr-btn--sm is-on"
        [appAssign]="action('reverse')"
        [title]="'The track plays backwards: press to play it forwards' | t"
        (click)="performance.setReverse(deck(), false)"
      >
        REV
      </button>
    }
    @if (muted()) {
      <button
        class="pr-btn pr-btn--sm is-on"
        [appAssign]="action('mute')"
        [title]="'The channel is muted: press to hear it again' | t"
        (click)="engine.setMute(deck(), false)"
      >
        MUTE
      </button>
    }
    <button
      class="pr-btn pr-btn--sm pr-btn--icon"
      [class.is-on]="at()"
      [class.pr-over-veil]="at()"
      data-assign-ui
      (click)="toggle($any($event.currentTarget))"
      [title]="'Deck options: motor, reverse, mute, repeat, tempo range, fader start, eject' | t"
      [attr.aria-label]="
        'Deck options: motor, reverse, mute, repeat, tempo range, fader start, eject' | t
      "
    >
      ⋯
    </button>
    @if (at(); as at) {
      <!-- the menu opened while assigning, its keys given theirs; the veil closes it -->
      <div
        class="pr-veil"
        data-assign-ui
        (dismiss)="close()"
        (contextmenu)="$event.preventDefault(); close()"
      ></div>
      <app-deck-options
        class="menu pr-popover"
        animate.enter="pr-pop-in"
        animate.leave="pr-pop-out"
        [style.left.px]="at.left"
        [style.right.px]="at.right"
        [style.top.px]="at.top"
        [style.bottom.px]="at.bottom"
        [deck]="deck()"
        (ejected)="close()"
      />
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
    /* as tall as the way the deck is driven beside them, the menu's key square */
    .pr-btn {
      height: 28px;
    }
    .pr-btn.pr-btn--icon {
      width: 28px;
    }
    .pr-btn:not(.pr-btn--icon) {
      padding: 0 7px;
      font: 600 10.5px / 1 var(--pr-font);
      letter-spacing: 0.04em;
    }
    .menu {
      position: fixed;
      width: 264px;
      padding: var(--pr-space-1) var(--pr-space-3);
      box-shadow: 0 16px 40px rgb(0 0 0 / 0.5);
    }
  `,
  imports: [DeckOptions, Dismiss, TPipe, AssignTarget],
})
export class DeckMenu {
  readonly deck = input.required<number>();
  /** The action `name` of this deck, for the assigning mode. */
  protected action(name: string): string {
    return deckAction(this.deck(), name);
  }
  /** Where the menu is open. */
  protected readonly at = signal<Popover | null>(null);
  protected readonly performance = inject(PerformanceService);
  protected readonly engine = inject(EngineService);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  /** The track plays backwards: not one its record holds in place, which keeps it going its
   * way. */
  protected readonly reversed = computed(() => {
    const status = this.engine.deckStatus(this.deck());
    return (status?.reverse ?? false) && (status?.canMove ?? false);
  });
  protected readonly muted = computed(() => this.engine.decks()[this.deck()].mute);

  constructor() {
    overlay(
      () => this.at() !== null,
      () => this.close(),
    );
  }

  /** Opens the menu at the pointer (a right click on the deck). */
  openAt(event: MouseEvent) {
    event.preventDefault();
    this.place(popoverAt(event.clientX, event.clientY));
  }

  protected close() {
    this.at.set(null);
  }

  /** Under its key, or over it in the lower half of the window; its right edge on the key's. */
  protected toggle(key: HTMLElement) {
    if (this.at()) return this.close();
    const r = key.getBoundingClientRect();
    const below = r.bottom < window.innerHeight / 2;
    this.place({
      left: null,
      right: window.innerWidth - r.right,
      top: below ? r.bottom + MENU_GAP_PX : null,
      bottom: below ? null : window.innerHeight - r.top + MENU_GAP_PX,
    });
  }

  /** The decks drawn at the scale of a screen below zoom the menu's offsets too. */
  private place(at: Popover) {
    const zoom = this.host.currentCSSZoom;
    const unzoomed = (px: number | null) => (px === null ? null : px / zoom);
    this.at.set({
      left: unzoomed(at.left),
      right: unzoomed(at.right),
      top: unzoomed(at.top),
      bottom: unzoomed(at.bottom),
    });
  }
}
