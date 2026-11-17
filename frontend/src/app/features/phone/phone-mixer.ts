import { Component, output, viewChild } from '@angular/core';
import { TPipe } from '../../core/i18n/t.pipe';
import { Dismiss, overlay } from '../../shared/overlay';
import { Sheet } from '../../shared/sheet';
import { MixerControls } from '../mixer/mixer-controls/mixer-controls';

/** The software mixer beside its channels, risen over the table: the master and the booth,
 * the limiter, the crossfader's curve, the headphones, the microphone and the aux line. */
@Component({
  selector: 'app-phone-mixer',
  template: `
    <div class="pr-veil pr-veil--dim" animate.enter="pr-fade-in" (dismiss)="sheet.close()"></div>
    <section
      class="pr-sheet panel"
      prSheet
      #sheet="prSheet"
      (dismissed)="closed.emit()"
      animate.enter="pr-rise-in"
      role="dialog"
      [attr.aria-label]="'Mixer' | t"
    >
      <button class="grip" (click)="sheet.close()" [attr.aria-label]="'Close' | t"></button>
      <h4>{{ 'Mixer' | t }}</h4>
      <app-mixer-controls />
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
  `,
  imports: [Dismiss, MixerControls, Sheet, TPipe],
})
export class PhoneMixer {
  readonly closed = output<void>();
  private readonly sheet = viewChild.required(Sheet);

  constructor() {
    overlay(
      () => true,
      () => this.sheet().close(),
    );
  }
}
