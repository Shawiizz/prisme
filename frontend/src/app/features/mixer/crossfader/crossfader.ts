import { Component, inject } from '@angular/core';
import { MixerService } from '../../../core/services/mixer.service';
import { TPipe } from '../../../core/i18n/t.pipe';
import { Fader } from '../../../shared/fader/fader';
import { MixerMenu } from '../mixer-menu/mixer-menu';

/** The crossfader of the software mixer, between its two sides, and the menu of the mixer. */
@Component({
  selector: 'app-crossfader',
  template: `
    <span class="pr-label">A</span>
    <app-fader
      centered
      [min]="-1"
      [max]="1"
      [value]="mixer.crossfader()"
      (valueChange)="mixer.setCrossfader($event)"
      [label]="'Crossfader' | t"
    />
    <span class="pr-label">B</span>
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
  `,
  host: { class: 'pr-panel' },
  imports: [Fader, MixerMenu, TPipe],
})
export class Crossfader {
  protected readonly mixer = inject(MixerService);
}
