import { Component, inject, input } from '@angular/core';
import { BANDS, Band } from '../../../core/models/engine';
import { MixerService } from '../../../core/services/mixer.service';
import { Knob } from '../../../shared/knob/knob';
import { eqGain, eqKnob, trimGain, trimKnob } from '../../../shared/mixer-scales';
import { t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';

/** A deck's channel of the software mixer, before its fader: trim, EQ with kills, filter. */
@Component({
  selector: 'app-channel-strip',
  template: `
    <app-knob
      label="TRIM"
      [labelTitle]="'Gain before the EQ (±12 dB)' | t"
      bipolar
      [value]="trimKnob()"
      (valueChange)="setTrim($event)"
    />
    @for (b of bands; track b.band) {
      <app-knob
        [label]="b.label"
        [labelTitle]="b.killTitle"
        bipolar
        [value]="eqKnob(b.band)"
        [active]="killed(b.band)"
        (valueChange)="setEq(b.band, $event)"
        (labelClick)="toggleKill(b.band)"
      />
    }
    <app-knob
      label="FILTER"
      [labelTitle]="'Left: low-pass, right: high-pass' | t"
      bipolar
      [value]="(strip().filter + 1) / 2"
      (valueChange)="mixer.setFilter(deck(), $event * 2 - 1)"
    />
  `,
  styles: `
    :host {
      display: flex;
      gap: 4px;
    }
  `,
  imports: [Knob, TPipe],
})
export class ChannelStrip {
  readonly deck = input.required<number>();
  protected readonly mixer = inject(MixerService);
  protected readonly bands: { band: Band; label: string; killTitle: string }[] = [
    { band: 'high', label: 'HI', killTitle: t('Click to kill the highs') },
    { band: 'mid', label: 'MID', killTitle: t('Click to kill the mids') },
    { band: 'low', label: 'LOW', killTitle: t('Click to kill the bass') },
  ];

  protected strip() {
    return this.mixer.strip(this.deck());
  }

  protected trimKnob(): number {
    return trimKnob(this.strip().trim);
  }

  protected setTrim(knob: number) {
    this.mixer.setTrim(this.deck(), trimGain(knob));
  }

  protected eqKnob(band: Band): number {
    return eqKnob(this.strip().eq[BANDS.indexOf(band)]);
  }

  protected setEq(band: Band, knob: number) {
    this.mixer.setEq(this.deck(), band, eqGain(knob));
  }

  protected killed(band: Band): boolean {
    return this.strip().kills[BANDS.indexOf(band)];
  }

  protected toggleKill(band: Band) {
    this.mixer.setKill(this.deck(), band, !this.killed(band));
  }
}
