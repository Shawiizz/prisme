import { Component, inject, input } from '@angular/core';
import { BANDS } from '../../../core/models/engine';
import { MixerService } from '../../../core/services/mixer.service';
import { Knob } from '../../../shared/knob/knob';
import { readout } from '../../../shared/readout';
import { Text, t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';
import type { Band, Knob as KnobView, StripKnobs } from '../../../core/models/backend';

/** A deck's channel of the software mixer, before its fader: EQ with kills, trim, filter. The
 * three bands come first, side by side as on a mixer. */
@Component({
  selector: 'app-channel-strip',
  template: `
    @for (b of bands; track b.band) {
      <app-knob
        [label]="b.label"
        [labelTitle]="b.killTitle()"
        bipolar
        [readout]="read(eq(b.band))"
        [value]="eq(b.band).at"
        [active]="killed(b.band)"
        (valueChange)="setEq(b.band, $event)"
        (labelClick)="toggleKill(b.band)"
      />
    }
    @if (trim()) {
      <app-knob
        label="TRIM"
        [labelTitle]="'Gain before the EQ (±12 dB)' | t"
        bipolar
        [readout]="read(knobs().trim)"
        [value]="knobs().trim.at"
        (valueChange)="setTrim($event)"
      />
    }
    <app-knob
      label="FILTER"
      [labelTitle]="'Left: low-pass, right: high-pass' | t"
      bipolar
      class="filter"
      [readout]="read(knobs().filter)"
      [value]="knobs().filter.at"
      (valueChange)="setFilter($event)"
    />
  `,
  styles: `
    :host {
      display: flex;
      gap: 4px;
      --knob-readout-width: 8ch;
    }
    .filter {
      --knob-readout-width: 9ch;
    }
  `,
  imports: [Knob, TPipe],
})
export class ChannelStrip {
  readonly deck = input.required<number>();
  /** With its trim knob (a phone leaves it out). */
  readonly trim = input(true);
  protected readonly mixer = inject(MixerService);
  protected readonly read = (knob: KnobView) => readout(knob.reads);
  protected readonly bands: { band: Band; label: string; killTitle: Text }[] = [
    {
      band: 'high',
      label: 'HI',
      killTitle: () => t('Click to kill the highs'),
    },
    {
      band: 'mid',
      label: 'MID',
      killTitle: () => t('Click to kill the mids'),
    },
    {
      band: 'low',
      label: 'LOW',
      killTitle: () => t('Click to kill the bass'),
    },
  ];

  protected strip() {
    return this.mixer.strip(this.deck());
  }

  protected knobs(): StripKnobs {
    return this.mixer.stripKnobs(this.deck());
  }

  protected setTrim(knob: number) {
    this.mixer.setTrim(this.deck(), knob);
  }

  protected eq(band: Band): KnobView {
    return this.knobs().eq[BANDS.indexOf(band)];
  }

  protected setEq(band: Band, knob: number) {
    this.mixer.setEq(this.deck(), band, knob);
  }

  protected setFilter(knob: number) {
    this.mixer.setFilter(this.deck(), knob);
  }

  protected killed(band: Band): boolean {
    return this.strip().kills[BANDS.indexOf(band)];
  }

  protected toggleKill(band: Band) {
    this.mixer.toggleKill(this.deck(), band);
  }
}
