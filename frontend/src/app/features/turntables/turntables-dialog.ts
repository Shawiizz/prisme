import { Component, output } from '@angular/core';
import { DECK_COUNT } from '../../core/models/engine';
import { TurntablePanel } from './turntable-panel';
import { TPipe } from '../../core/i18n/t.pipe';

/** Both turntables side by side, to check and set up vinyl control. */
@Component({
  selector: 'app-turntables-dialog',
  imports: [TurntablePanel, TPipe],
  template: `
    <div class="pr-backdrop" (click)="closed.emit()">
      <div
        class="pr-dialog"
        role="dialog"
        aria-modal="true"
        [attr.aria-label]="'Turntables' | t"
        (click)="$event.stopPropagation()"
      >
        <header class="pr-dialog__head">
          <h2>{{ 'Turntables' | t }}</h2>
          <button
            class="pr-btn pr-btn--ghost pr-btn--icon"
            [attr.aria-label]="'Close' | t"
            (click)="closed.emit()"
          >
            ×
          </button>
        </header>
        <div class="pr-dialog__body decks">
          @for (d of decks; track d) {
            <app-turntable-panel [deck]="d" />
          }
        </div>
      </div>
    </div>
  `,
  styles: `
    .pr-dialog {
      width: min(760px, calc(100vw - 48px));
    }
    .decks {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: var(--pr-space-5);
    }
  `,
})
export class TurntablesDialog {
  readonly closed = output<void>();
  protected readonly decks = Array.from({ length: DECK_COUNT }, (_, i) => i);
}
