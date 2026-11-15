import { Component, DestroyRef, computed, inject, input } from '@angular/core';
import { PHONE } from '../../../core/platform';
import { EngineService } from '../../../core/services/engine.service';
import { PerformanceService } from '../../../core/services/performance.service';
import { TPipe } from '../../../core/i18n/t.pipe';
import { Press } from '../../../shared/press';

/**
 * What a deck does now and then rather than all the time, a row each: its motor (a brake, a
 * soft start, a spinback while held), the track repeated.
 */
@Component({
  selector: 'app-deck-options',
  template: `
    <div class="row">
      <span>{{ 'Motor' | t }}</span>
      <div class="keys">
        <button
          class="pr-btn pr-btn--sm"
          [class.pr-reach]="PHONE"
          [disabled]="!movable()"
          [title]="'Brake: the deck slows to a stop; stopped, it starts softly' | t"
          (click)="performance.brake(deck())"
        >
          BRAKE
        </button>
        <button
          class="pr-btn pr-btn--sm"
          [class.pr-reach]="PHONE"
          [disabled]="!movable()"
          [title]="'Spinback: the record spun back while held, then it plays on' | t"
          (press)="spinBack(true)"
          (release)="spinBack(false)"
        >
          SPINBACK
        </button>
      </div>
    </div>
    <div class="row">
      <span>{{ 'Repeat the track' | t }}</span>
      <button
        class="pr-switch"
        role="switch"
        [attr.aria-checked]="settings().repeat"
        [attr.aria-label]="'Repeat the track' | t"
        [title]="'At the end, the track plays on from its start' | t"
        (click)="engine.setRepeat(deck(), !settings().repeat)"
      >
        <span></span>
      </button>
    </div>
  `,
  styles: `
    :host {
      display: grid;
    }
    .row {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--pr-space-3);
      min-height: 44px;
      font-size: var(--pr-size-small);
      color: var(--pr-text-2);
      & + & {
        border-top: 1px solid var(--pr-line);
      }
    }
    .keys {
      display: flex;
      align-items: center;
      gap: var(--pr-space-1);
      .pr-btn {
        padding: 0 7px;
        font: 600 10.5px / 1 var(--pr-font);
        letter-spacing: 0.04em;
      }
    }
    // a finger's room between the rows
    :host-context(.phone) .row {
      min-height: 60px;
    }
  `,
  imports: [Press, TPipe],
})
export class DeckOptions {
  readonly deck = input.required<number>();

  protected readonly engine = inject(EngineService);
  protected readonly performance = inject(PerformanceService);
  protected readonly PHONE = PHONE;
  protected readonly settings = computed(() => this.engine.decks()[this.deck()]);

  /** The motor moves a track that plays by itself: a record drives its own. */
  protected readonly movable = computed(
    () =>
      !!this.engine.status()?.decks[this.deck()]?.loaded &&
      !this.settings().vinylEnabled &&
      !this.settings().passthrough,
  );

  /** The deck spun back: the one let go, even if the menu changed decks meanwhile. */
  private spun: number | null = null;

  constructor() {
    inject(DestroyRef).onDestroy(() => this.spinBack(false));
  }

  protected spinBack(held: boolean) {
    if (held) {
      this.spun = this.deck();
      this.performance.spinBack(this.spun, true);
    } else if (this.spun !== null) {
      this.performance.spinBack(this.spun, false);
      this.spun = null;
    }
  }
}
