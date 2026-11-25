import { Component, signal } from '@angular/core';
import { TPipe } from '../../../core/i18n/t.pipe';
import { Dismiss, overlay } from '../../../shared/overlay';
import { MixerControls } from '../mixer-controls/mixer-controls';

/**
 * What is set once an evening on the software mixer, off the screen until asked for: the
 * master, the limiter, the crossfader's curve, the headphones, the microphone and the aux line.
 */
@Component({
  selector: 'app-mixer-menu',
  template: `
    <button
      class="pr-btn pr-btn--sm pr-btn--icon"
      [class.is-on]="open()"
      (click)="open.set(!open())"
      [title]="'Master, limiter, headphones, microphone and aux line' | t"
      [attr.aria-label]="'Master, limiter, headphones, microphone and aux line' | t"
    >
      ⋯
    </button>
    @if (open()) {
      <div class="pr-veil" (dismiss)="open.set(false)"></div>
      <app-mixer-controls
        class="menu pr-popover"
        animate.enter="pr-pop-in"
        animate.leave="pr-pop-out"
      />
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
      width: 288px;
      padding: var(--pr-space-2) var(--pr-space-3);
      box-shadow: 0 16px 40px rgb(0 0 0 / 0.5);
    }
  `,
  imports: [MixerControls, TPipe, Dismiss],
})
export class MixerMenu {
  protected readonly open = signal(false);

  constructor() {
    overlay(this.open, () => this.open.set(false));
  }
}
