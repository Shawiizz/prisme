import { Component, computed, inject } from '@angular/core';
import { LiveSource } from '../../../core/models/engine';
import { PHONE } from '../../../core/platform';
import { EngineService } from '../../../core/services/engine.service';
import { MixerService } from '../../../core/services/mixer.service';
import { Text, t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';
import { Knob } from '../../../shared/knob/knob';
import { levelMeter } from '../../../shared/level';
import { balanceKnob, liveKnob, readouts } from '../../../shared/mixer-scales';

/** The headphone volume knob goes up to +3.5 dB. */
const MAX_HEADPHONE_GAIN = 1.5;
const METER_SEGMENTS = 12;

/** What names a live source and its keys. */
const LIVE_TEXTS: Record<LiveSource, Record<'label' | 'on' | 'cue' | 'level', Text>> = {
  mic: {
    label: () => t('Microphone'),
    on: () => t('Microphone on / off'),
    cue: () => t('Microphone in the headphones'),
    level: () => t('Microphone level'),
  },
  aux: {
    label: () => t('Aux line'),
    on: () => t('Aux line on / off'),
    cue: () => t('Aux line in the headphones'),
    level: () => t('Aux line level'),
  },
};

/**
 * What is set on the software mixer beside its channels, a row each: the master's level and
 * balance and the booth's level, the limiter, the crossfader's curve and sides, the headphones (blended or split),
 * and the microphone
 * and the aux line plugged in (each with its level, its key and its headphones).
 */
@Component({
  selector: 'app-mixer-controls',
  template: `
    <div class="row">
      <span>{{ 'Master' | t }}</span>
      <div class="knobs">
        <app-knob
          [small]="!PHONE"
          label="LEVEL"
          [labelTitle]="'Master level' | t"
          [defaultValue]="1"
          [format]="readouts.gain"
          [value]="mixer.mainGain()"
          (valueChange)="mixer.setMainGain($event)"
        />
        <app-knob
          [small]="!PHONE"
          bipolar
          label="BAL"
          [labelTitle]="'Master balance' | t"
          [defaultValue]="0.5"
          [format]="readouts.balance"
          [value]="balanceKnob(mixer.balance())"
          (valueChange)="mixer.setBalance($event)"
        />
        @if (mixer.boothInUse() !== null) {
          <app-knob
            [small]="!PHONE"
            label="BOOTH"
            [labelTitle]="'Booth level' | t"
            [defaultValue]="1"
            [format]="readouts.gain"
            [value]="mixer.boothGain()"
            (valueChange)="mixer.setBoothGain($event)"
          />
        }
      </div>
    </div>
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
      <span>{{ 'Crossfader' | t }}</span>
      <div class="keys">
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
            {{ 'SMOOTH' | t }}
          </button>
          <button
            role="radio"
            [attr.aria-checked]="mixer.crossfaderCurve() === 'sharp'"
            (click)="mixer.setCrossfaderCurve('sharp')"
          >
            {{ 'SHARP' | t }}
          </button>
        </div>
        <button
          class="pr-btn pr-btn--sm"
          [class.pr-reach]="PHONE"
          [class.is-on]="mixer.crossfaderReverse()"
          [attr.aria-pressed]="mixer.crossfaderReverse()"
          [title]="'Crossfader sides swapped' | t"
          (click)="mixer.setCrossfaderReverse(!mixer.crossfaderReverse())"
        >
          REV
        </button>
      </div>
    </div>
    <div class="row">
      <span>{{ 'Headphones' | t }}</span>
      <div class="knobs">
        <app-knob
          [small]="!PHONE"
          label="CUE/MST"
          [labelTitle]="'Headphones: cued decks or master' | t"
          [defaultValue]="0"
          [format]="readouts.percent"
          [value]="mixer.cueMix()"
          (valueChange)="mixer.setCueMix($event)"
        />
        <app-knob
          [small]="!PHONE"
          label="PHONES"
          [labelTitle]="'Headphone volume' | t"
          [defaultValue]="1 / MAX_HEADPHONE_GAIN"
          [format]="phonesReadout"
          [value]="mixer.headphoneGain() / MAX_HEADPHONE_GAIN"
          (valueChange)="mixer.setHeadphoneLevel($event)"
        />
        <button
          class="pr-btn pr-btn--sm split"
          [class.pr-reach]="PHONE"
          [class.is-on]="mixer.splitCue()"
          [attr.aria-pressed]="mixer.splitCue()"
          [title]="'Split headphones: cue left, master right' | t"
          (click)="mixer.setSplitCue(!mixer.splitCue())"
        >
          SPLIT
        </button>
      </div>
    </div>
    @for (live of lives(); track live.source) {
      <div class="row">
        <span class="name">
          {{ live.label }}
          <span class="pr-meter" [title]="'Level of the input' | t">
            @for (segment of meter(live.source); track $index) {
              <i [class]="segment"></i>
            }
          </span>
        </span>
        <div class="keys">
          <button
            class="pr-btn pr-btn--sm"
            [class.pr-reach]="PHONE"
            [class.is-on]="live.state.on"
            [attr.aria-pressed]="live.state.on"
            [title]="live.onTitle"
            (click)="mixer.toggleLiveOn(live.source)"
          >
            ON
          </button>
          @if (live.source === 'mic') {
            <button
              class="pr-btn pr-btn--sm"
              [class.pr-reach]="PHONE"
              [class.is-on]="mixer.talkover()"
              [attr.aria-pressed]="mixer.talkover()"
              [title]="'Talkover: the music ducks while the microphone is on' | t"
              (click)="mixer.setTalkover(!mixer.talkover())"
            >
              TALK
            </button>
          }
          <button
            class="pr-btn pr-btn--sm pr-btn--icon"
            [class.pr-reach]="PHONE"
            [class.is-on]="live.state.cue"
            [attr.aria-pressed]="live.state.cue"
            [title]="live.cueTitle"
            [attr.aria-label]="live.cueTitle"
            (click)="mixer.toggleLiveCue(live.source)"
          >
            <svg viewBox="0 0 16 16" aria-hidden="true">
              <path
                d="M2.5 10V8a5.5 5.5 0 0 1 11 0v2"
                fill="none"
                stroke="currentColor"
                stroke-width="1.5"
              />
              <rect x="1.5" y="9" width="3.5" height="5" rx="1" fill="currentColor" />
              <rect x="11" y="9" width="3.5" height="5" rx="1" fill="currentColor" />
            </svg>
          </button>
          <app-knob
            [small]="!PHONE"
            label="LEVEL"
            [labelTitle]="live.levelTitle"
            [defaultValue]="liveKnob(1)"
            [format]="readouts.live"
            [value]="liveKnob(live.state.level)"
            (valueChange)="mixer.setLiveLevel(live.source, $event)"
          />
        </div>
      </div>
    }
  `,
  styles: `
    :host {
      display: grid;
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
    .knobs,
    .keys {
      display: flex;
      align-items: center;
      gap: var(--pr-space-1);
      padding-block: var(--pr-space-1);
      --knob-label-width: 50px;
    }
    .split {
      padding: 0 7px;
      font: 600 10.5px / 1 var(--pr-font);
      letter-spacing: 0.04em;
    }
    .keys {
      --knob-label-width: 40px;
      .pr-btn:not(.pr-btn--icon) {
        padding: 0 7px;
        font: 600 10.5px / 1 var(--pr-font);
        letter-spacing: 0.04em;
      }
      svg {
        width: 14px;
        height: 14px;
      }
    }
    // a finger's room between the rows
    :host-context(.phone) .row {
      min-height: 60px;
    }
    .name {
      display: grid;
      gap: 5px;
      .pr-meter {
        width: 56px;
        height: 3px;
      }
    }
  `,
  imports: [Knob, TPipe],
})
export class MixerControls {
  protected readonly mixer = inject(MixerService);
  private readonly engine = inject(EngineService);
  protected readonly PHONE = PHONE;
  protected readonly MAX_HEADPHONE_GAIN = MAX_HEADPHONE_GAIN;
  protected readonly readouts = readouts;
  protected readonly phonesReadout = (knob: number) => readouts.gain(knob * MAX_HEADPHONE_GAIN);
  protected readonly balanceKnob = balanceKnob;
  protected readonly liveKnob = liveKnob;

  private readonly meters: Record<LiveSource, () => string[]> = {
    mic: levelMeter(() => this.engine.status()?.micPeak ?? 0, METER_SEGMENTS),
    aux: levelMeter(() => this.engine.status()?.auxPeak ?? 0, METER_SEGMENTS),
  };

  /** The live sources plugged in, on inputs the turntables leave free. */
  protected readonly lives = computed(() =>
    (['mic', 'aux'] as const)
      .filter((source) => this.mixer.live(source)().inUse)
      .map((source) => {
        const texts = LIVE_TEXTS[source];
        return {
          source,
          state: this.mixer.live(source)(),
          label: texts.label(),
          onTitle: texts.on(),
          cueTitle: texts.cue(),
          levelTitle: texts.level(),
        };
      }),
  );

  protected meter(source: LiveSource): string[] {
    return this.meters[source]();
  }
}
