import { Component, computed, inject, input } from '@angular/core';
import { EngineService } from '../../../core/services/engine.service';
import { MixerService } from '../../../core/services/mixer.service';
import { TPipe } from '../../../core/i18n/t.pipe';
import { Fader } from '../../../shared/fader/fader';
import { levelSegments } from '../../../shared/level';
import { deckLetter } from '../../../shared/time';
import { ChannelStrip } from '../channel-strip/channel-strip';

const METER_SEGMENTS = 24;

/**
 * A deck's channel of the software mixer, at the end of its lane: what is touched while
 * mixing (trim, EQ, filter, the headphones, the fader and its level).
 */
@Component({
  selector: 'app-channel-slot',
  template: `
    <div class="strip">
      <app-channel-strip [deck]="deck()" />
      <button
        class="pr-btn pr-btn--icon cue"
        [class.is-on]="cued()"
        (click)="mixer.setDeckCue(deck(), !cued())"
        [title]="'Hear this deck in the headphones' | t"
        [attr.aria-label]="'Hear this deck in the headphones' | t"
      >
        <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
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
    </div>
    <div class="pr-meter pr-meter--vertical">
      @for (segment of level(); track $index) {
        <i [class]="segment"></i>
      }
    </div>
    <app-fader
      orientation="vertical"
      [value]="volume()"
      [defaultValue]="1"
      (valueChange)="mixer.setVolume(deck(), $event)"
      [label]="'Volume of deck {deck}' | t: { deck: letter() }"
    />
  `,
  styles: `
    :host {
      display: flex;
      align-items: stretch;
      gap: 6px;
      padding: 8px 12px 8px 8px;
      min-width: 0;
    }
    .strip {
      margin-right: auto;
      align-self: center;
      display: grid;
      grid-template-columns: repeat(3, 44px);
      gap: 6px 0;
      justify-items: center;
      align-items: start;
      --knob-size: 28px;
      --knob-label-width: 40px;
      app-channel-strip {
        display: contents;
      }
    }
    .cue {
      width: 28px;
      height: 28px;
      padding: 0;
    }
    .pr-meter {
      width: 5px;
    }
    app-fader {
      height: auto;
    }
  `,
  host: { class: 'pr-panel' },
  imports: [ChannelStrip, Fader, TPipe],
})
export class ChannelSlot {
  readonly deck = input.required<number>();

  private readonly engine = inject(EngineService);
  protected readonly mixer = inject(MixerService);
  protected readonly letter = computed(() => deckLetter(this.deck()));
  protected readonly cued = computed(() => this.engine.decks()[this.deck()].cue);
  protected readonly volume = computed(() => this.engine.decks()[this.deck()].volume);
  protected readonly level = computed(() =>
    levelSegments(this.engine.status()?.decks[this.deck()]?.peak ?? 0, METER_SEGMENTS),
  );
}
