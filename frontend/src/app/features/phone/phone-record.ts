import { Component, inject, output, viewChild } from '@angular/core';
import { TPipe } from '../../core/i18n/t.pipe';
import { RecordOption, RecordingService } from '../../core/services/recording.service';
import { Dismiss, overlay } from '../../shared/overlay';
import { Sheet } from '../../shared/sheet';

/** What to record, risen from the Rec key: the software mixer's master, and the inputs of the
 * sound card, each with what it holds. */
@Component({
  selector: 'app-phone-record',
  template: `
    <div class="pr-veil pr-veil--dim" animate.enter="pr-fade-in" (dismiss)="sheet.close()"></div>
    <section
      class="pr-sheet panel"
      prSheet
      #sheet="prSheet"
      (dismissed)="closed.emit()"
      animate.enter="pr-rise-in"
      role="dialog"
      [attr.aria-label]="'Record the mix' | t"
    >
      <button class="grip" (click)="sheet.close()" [attr.aria-label]="'Close' | t"></button>
      <h4>{{ 'Record the mix' | t }}</h4>
      @for (o of recording.options(); track o.label) {
        <button class="option" (click)="record(o)">
          <span class="label">{{ o.label }}</span>
          <span class="help">{{ o.title }}</span>
        </button>
      } @empty {
        <p class="help">{{ 'Nothing to record: no software mixer, no input' | t }}</p>
      }
    </section>
  `,
  styles: `
    .panel {
      max-height: 92%;
      padding: 0 16px calc(18px + env(safe-area-inset-bottom));
    }
    .grip {
      display: block;
      width: 100%;
      height: 20px;
      padding: 0;
      border: 0;
      background: none;
      &::after {
        content: '';
        display: block;
        width: 36px;
        height: 3px;
        margin: 0 auto;
        background: var(--pr-line-2);
      }
    }
    h4 {
      margin: 12px 0 4px;
      font: 600 var(--pr-size-label) / 1 var(--pr-font);
      letter-spacing: 0.08em;
      text-transform: uppercase;
      color: var(--pr-text-3);
    }
    .option {
      display: grid;
      gap: 3px;
      width: 100%;
      min-height: 60px;
      padding: 10px 0;
      border: 0;
      background: none;
      font: inherit;
      text-align: left;
      color: var(--pr-text);
      & + & {
        border-top: 1px solid var(--pr-line);
      }
    }
    .label {
      font-size: 16px;
      font-weight: 600;
    }
    .help {
      margin: 0;
      font-size: 13px;
      line-height: 1.4;
      color: var(--pr-text-3);
    }
  `,
  imports: [Dismiss, Sheet, TPipe],
})
export class PhoneRecord {
  readonly closed = output<void>();
  private readonly sheet = viewChild.required(Sheet);
  protected readonly recording = inject(RecordingService);

  constructor() {
    overlay(
      () => true,
      () => this.sheet().close(),
    );
  }

  protected record(option: RecordOption) {
    this.sheet().close();
    this.recording.start(option.source);
  }
}
