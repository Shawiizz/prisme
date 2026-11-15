import { Component, computed, inject, input, output } from '@angular/core';
import { EngineService } from '../../core/services/engine.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { deckLetter } from '../../shared/time';

/**
 * The head of what floats over the table of a phone: the deck's letter, lit while it plays,
 * the name of what floats, what is projected after it, and the key that closes it.
 */
@Component({
  selector: 'app-phone-float-head',
  template: `
    <span class="pr-tile" [class.pr-tile--outline]="!playing()">{{ letter() }}</span>
    <b>{{ name() }}</b>
    <ng-content />
    <button
      class="pr-btn pr-btn--sm pr-btn--icon pr-btn--ghost"
      (click)="closed.emit()"
      [attr.aria-label]="'Close' | t"
    >
      ×
    </button>
  `,
  styles: `
    :host {
      display: flex;
      align-items: center;
      gap: 8px;
    }
    b {
      flex: 1;
    }
    .pr-tile {
      width: 22px;
      height: 22px;
      font-size: 12px;
    }
  `,
  imports: [TPipe],
})
export class PhoneFloatHead {
  readonly deck = input.required<number>();
  readonly name = input.required<string>();
  readonly closed = output<void>();

  private readonly engine = inject(EngineService);
  protected readonly letter = computed(() => deckLetter(this.deck()));
  protected readonly playing = computed(
    () => this.engine.status()?.decks[this.deck()]?.playing ?? false,
  );
}
