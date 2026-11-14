import { Component, computed, inject, input } from '@angular/core';
import { EngineService } from '../../core/services/engine.service';
import { MixerService } from '../../core/services/mixer.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { Fader } from '../../shared/fader/fader';
import { levelMeter } from '../../shared/level';
import { deckLetter } from '../../shared/time';
import { ChannelStrip } from '../mixer/channel-strip/channel-strip';

const METER_SEGMENTS = 16;
/** The marks beside the fader, a long one every five. */
const MARKS = Array.from({ length: 11 }, (_, i) => i % 5 === 0);

/**
 * A deck's channel on the table of a phone, as a mixer lays one out: its EQ and filter, then
 * its fader between its marks and its level, the headphones beside it. The `b` side is its
 * mirror.
 */
@Component({
  selector: 'app-phone-channel',
  template: `
    <app-channel-strip [deck]="deck()" [trim]="false" />
    <button
      class="pr-btn pr-btn--icon cue"
      [class.is-on]="cued()"
      [attr.aria-pressed]="cued()"
      (click)="mixer.setDeckCue(deck(), !cued())"
      [title]="'Hear this deck in the headphones' | t"
      [attr.aria-label]="'Hear this deck in the headphones' | t"
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
    <div class="strip">
      <div class="marks">
        @for (long of marks; track $index) {
          <i [class.long]="long"></i>
        }
      </div>
      <app-fader
        touch
        orientation="vertical"
        [value]="volume()"
        [defaultValue]="1"
        (valueChange)="mixer.setVolume(deck(), $event)"
        [label]="'Volume of deck {deck}' | t: { deck: letter() }"
      />
      <div class="pr-meter pr-meter--vertical">
        @for (segment of level(); track $index) {
          <i [class]="segment"></i>
        }
      </div>
    </div>
  `,
  styleUrl: './phone-channel.scss',
  host: { '[class]': 'side()' },
  imports: [ChannelStrip, Fader, TPipe],
})
export class PhoneChannel {
  readonly deck = input.required<number>();
  readonly side = input<'a' | 'b'>('a');

  private readonly engine = inject(EngineService);
  protected readonly mixer = inject(MixerService);
  protected readonly marks = MARKS;
  protected readonly letter = computed(() => deckLetter(this.deck()));
  protected readonly cued = computed(() => this.engine.decks()[this.deck()].cue);
  protected readonly volume = computed(() => this.engine.decks()[this.deck()].volume);
  protected readonly level = levelMeter(
    () => this.engine.status()?.decks[this.deck()]?.peak ?? 0,
    METER_SEGMENTS,
  );
}
