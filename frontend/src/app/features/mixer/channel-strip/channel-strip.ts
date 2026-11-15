import { Component, computed, inject, input } from '@angular/core';
import { BANDS, Band } from '../../../core/models/engine';
import { MixerService } from '../../../core/services/mixer.service';
import { PreferencesService } from '../../../core/services/preferences.service';
import { Knob } from '../../../shared/knob/knob';
import {
  bandKnob,
  eqGain,
  filterAmount,
  filterKnob,
  readouts,
  trimGain,
  trimKnob,
} from '../../../shared/mixer-scales';
import { Text, t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';

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
        [magnetic]="magnetic()"
        [format]="readouts.eq"
        [value]="eqKnob(b.band)"
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
        [magnetic]="magnetic()"
        [format]="readouts.trim"
        [value]="trimKnob()"
        (valueChange)="setTrim($event)"
      />
    }
    <app-knob
      label="FILTER"
      [labelTitle]="'Left: low-pass, right: high-pass' | t"
      bipolar
      [magnetic]="magnetic()"
      [format]="readouts.filter"
      [value]="filterKnob()"
      (valueChange)="setFilter($event)"
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
  /** With its trim knob (a phone leaves it out). */
  readonly trim = input(true);
  protected readonly mixer = inject(MixerService);
  private readonly preferences = inject(PreferencesService);
  /** The knobs hold at their middle, as the DJ chose. */
  protected readonly magnetic = computed(() => this.preferences.dj().knobMagnet);
  protected readonly readouts = readouts;
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

  protected trimKnob(): number {
    return trimKnob(this.strip().trim);
  }

  protected setTrim(knob: number) {
    this.mixer.setTrim(this.deck(), trimGain(knob));
  }

  protected eqKnob(band: Band): number {
    return bandKnob(this.strip(), band);
  }

  protected setEq(band: Band, knob: number) {
    this.mixer.setEq(this.deck(), band, eqGain(knob));
  }

  protected filterKnob(): number {
    return filterKnob(this.strip().filter);
  }

  protected setFilter(knob: number) {
    this.mixer.setFilter(this.deck(), filterAmount(knob));
  }

  protected killed(band: Band): boolean {
    return this.strip().kills[BANDS.indexOf(band)];
  }

  protected toggleKill(band: Band) {
    this.mixer.setKill(this.deck(), band, !this.killed(band));
  }
}
