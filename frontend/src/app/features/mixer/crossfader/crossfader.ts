import { Component, inject } from '@angular/core';
import { MixerService } from '../../../core/services/mixer.service';
import { TPipe } from '../../../core/i18n/t.pipe';
import { Fader } from '../../../shared/fader/fader';
import { MixerMenu } from '../mixer-menu/mixer-menu';

/** The crossfader of the software mixer, between its two sides, the microphone's key when one
 * is plugged in, and the menu of the mixer. */
@Component({
  selector: 'app-crossfader',
  template: `
    <span class="pr-label">A</span>
    <app-fader
      centered
      [value]="mixer.knobs()?.crossfader ?? 0.5"
      (valueChange)="mixer.setCrossfader($event)"
      [label]="'Crossfader' | t"
    />
    <span class="pr-label">B</span>
    @if (mic().inUse) {
      <button
        class="pr-btn pr-btn--sm mic"
        [class.is-on]="mic().on"
        [attr.aria-pressed]="mic().on"
        [title]="
          mixer.talkover()
            ? ('Microphone on / off: the music ducks while it is on' | t)
            : ('Microphone on / off' | t)
        "
        (click)="mixer.toggleLiveOn('mic')"
      >
        MIC
      </button>
    }
    <app-mixer-menu />
  `,
  styles: `
    :host {
      display: flex;
      align-items: center;
      gap: var(--pr-space-2);
      padding: 0 var(--pr-space-2) 0 var(--pr-space-3);
    }
    app-fader {
      flex: 1;
    }
    .mic {
      padding: 0 8px;
      font: 600 10.5px / 1 var(--pr-font);
      letter-spacing: 0.06em;
    }
  `,
  host: { class: 'pr-panel', '[class.live]': 'mic().inUse' },
  imports: [Fader, MixerMenu, TPipe],
})
export class Crossfader {
  protected readonly mixer = inject(MixerService);
  protected readonly mic = this.mixer.mic;
}
