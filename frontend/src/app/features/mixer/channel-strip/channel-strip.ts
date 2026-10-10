import { Component, inject, input } from '@angular/core';
import { BANDS, Band } from '../../../core/models/engine';
import { MixerService } from '../../../core/services/mixer.service';
import { Knob } from '../../../shared/knob/knob';
import { eqGain, eqKnob, trimGain, trimKnob } from '../../../shared/mixer-scales';

/** A deck's channel of the software mixer, before its fader: trim, EQ with kills, filter. */
@Component({
  selector: 'app-channel-strip',
  template: `
    <app-knob
      label="TRIM"
      labelTitle="Gain before the EQ (±12 dB)"
      bipolar
      [value]="trimKnob()"
      (valueChange)="setTrim($event)"
    />
    @for (b of bands; track b.band) {
      <app-knob
        [label]="b.label"
        [labelTitle]="'Click to kill the ' + b.name"
        bipolar
        [value]="eqKnob(b.band)"
        [active]="killed(b.band)"
        (valueChange)="setEq(b.band, $event)"
        (labelClick)="toggleKill(b.band)"
      />
    }
    <app-knob
      label="FILTER"
      labelTitle="Left: low-pass, right: high-pass"
      bipolar
      [value]="(strip().filter + 1) / 2"
      (valueChange)="mixer.setFilter(deck(), $event * 2 - 1)"
    />
  `,
  styles: `
    :host {
      display: flex;
      gap: 6px;
    }
  `,
  imports: [Knob],
})
export class ChannelStrip {
  readonly deck = input.required<number>();
  protected readonly mixer = inject(MixerService);
  protected readonly bands: { band: Band; label: string; name: string }[] = [
    { band: 'high', label: 'HI', name: 'highs' },
    { band: 'mid', label: 'MID', name: 'mids' },
    { band: 'low', label: 'LOW', name: 'bass' },
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
