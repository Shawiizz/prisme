import { Component, output } from '@angular/core';
import { DECK_COUNT } from '../../core/models/engine';
import { TurntablePanel } from './turntable-panel';
import { TPipe } from '../../core/i18n/t.pipe';

/** Both turntables side by side, to check and set up vinyl control. */
@Component({
  selector: 'app-turntables-dialog',
  imports: [TurntablePanel, TPipe],
  template: `
    <div class="backdrop" (click)="closed.emit()">
      <div class="dialog" (click)="$event.stopPropagation()">
        <h2>{{ 'Turntables' | t }}</h2>
        <div class="decks">
          @for (d of decks; track d) {
            <app-turntable-panel [deck]="d" />
          }
        </div>
        <div class="buttons">
          <button (click)="closed.emit()">{{ 'Close' | t }}</button>
        </div>
      </div>
    </div>
  `,
  styles: `
    .backdrop {
      position: fixed;
      inset: 0;
      z-index: 10;
      display: flex;
      align-items: center;
      justify-content: center;
      background: rgba(4, 5, 9, 0.7);
    }
    .dialog {
      width: 640px;
      display: flex;
      flex-direction: column;
      gap: 16px;
      padding: 20px;
      background: var(--panel);
      border: 1px solid var(--line-strong);
      border-radius: 4px;
    }
    h2 {
      margin: 0;
      font-size: 16px;
      font-weight: 600;
    }
    .decks {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 28px;
    }
    .buttons {
      display: flex;
      justify-content: flex-end;
    }
  `,
})
export class TurntablesDialog {
  readonly closed = output<void>();
  protected readonly decks = Array.from({ length: DECK_COUNT }, (_, i) => i);
}
