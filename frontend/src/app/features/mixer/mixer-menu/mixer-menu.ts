import { Component, inject, signal } from '@angular/core';
import { MixerService } from '../../../core/services/mixer.service';
import { TPipe } from '../../../core/i18n/t.pipe';
import { Knob } from '../../../shared/knob/knob';
import { readouts } from '../../../shared/mixer-scales';

/** The headphone volume knob goes up to +3.5 dB. */
const MAX_HEADPHONE_GAIN = 1.5;

/**
 * What is set once an evening on the software mixer, off the screen until asked for: the
 * master limiter, the crossfader's curve, and the headphones (cue/master blend, volume).
 */
@Component({
  selector: 'app-mixer-menu',
  template: `
    <button
      class="pr-btn pr-btn--sm pr-btn--icon"
      [class.is-on]="open()"
      (click)="open.set(!open())"
      [title]="'Limiter, crossfader curve and headphones' | t"
      [attr.aria-label]="'Limiter, crossfader curve and headphones' | t"
    >
      ⋯
    </button>
    @if (open()) {
      <div class="backdrop" (click)="open.set(false)"></div>
      <div class="menu pr-panel">
        <div class="row">
          <span>{{ 'Master limiter' | t }}</span>
          <button
            class="pr-switch"
            role="switch"
            [attr.aria-checked]="mixer.limiter()"
            [attr.aria-label]="'Master limiter' | t"
            [title]="'Master limiter: no clipping (adds 1 ms of latency)' | t"
            (click)="mixer.setLimiter(!mixer.limiter())"
          >
            <span></span>
          </button>
        </div>
        <div class="row">
          <span>{{ 'Crossfader curve' | t }}</span>
          <div
            class="pr-seg"
            role="radiogroup"
            [attr.aria-label]="'Crossfader curve' | t"
            [title]="'Crossfader curve: smooth for blending, sharp for scratching' | t"
          >
            <button
              role="radio"
              [attr.aria-checked]="mixer.crossfaderCurve() === 'smooth'"
              (click)="mixer.setCrossfaderCurve('smooth')"
            >
              SMOOTH
            </button>
            <button
              role="radio"
              [attr.aria-checked]="mixer.crossfaderCurve() === 'sharp'"
              (click)="mixer.setCrossfaderCurve('sharp')"
            >
              SHARP
            </button>
          </div>
        </div>
        <div class="row">
          <span>{{ 'Headphones' | t }}</span>
          <div class="knobs">
            <app-knob
              small
              label="CUE/MST"
              [labelTitle]="'Headphones: cued decks or master' | t"
              [defaultValue]="0"
              [format]="readouts.percent"
              [value]="mixer.cueMix()"
              (valueChange)="mixer.setCueMix($event)"
            />
            <app-knob
              small
              label="PHONES"
              [labelTitle]="'Headphone volume' | t"
              [defaultValue]="1 / MAX_HEADPHONE_GAIN"
              [format]="phonesReadout"
              [value]="mixer.headphoneGain() / MAX_HEADPHONE_GAIN"
              (valueChange)="mixer.setHeadphoneGain($event * MAX_HEADPHONE_GAIN)"
            />
          </div>
        </div>
      </div>
    }
  `,
  styles: `
    :host {
      position: relative;
      flex: none;
    }
    .backdrop {
      position: fixed;
      inset: 0;
      z-index: 20;
    }
    .menu {
      position: absolute;
      z-index: 21;
      top: calc(100% + 6px);
      right: 0;
      width: 264px;
      padding: var(--pr-space-2) var(--pr-space-3);
      display: grid;
      border-color: var(--pr-line-2);
      box-shadow: 0 16px 40px rgb(0 0 0 / 0.5);
    }
    .row {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--pr-space-3);
      min-height: 44px;
      font-size: var(--pr-size-small);
      color: var(--pr-text-2);
      & + & {
        border-top: 1px solid var(--pr-line);
      }
    }
    .knobs {
      display: flex;
      gap: var(--pr-space-1);
      padding-block: var(--pr-space-1);
      --knob-label-width: 50px;
    }
  `,
  imports: [Knob, TPipe],
})
export class MixerMenu {
  protected readonly mixer = inject(MixerService);
  protected readonly open = signal(false);
  protected readonly MAX_HEADPHONE_GAIN = MAX_HEADPHONE_GAIN;
  protected readonly readouts = readouts;
  protected readonly phonesReadout = (knob: number) => readouts.gain(knob * MAX_HEADPHONE_GAIN);
}
