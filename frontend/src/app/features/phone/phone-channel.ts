import { Component, computed, inject, input } from '@angular/core';
import { EngineService } from '../../core/services/engine.service';
import { MixerService } from '../../core/services/mixer.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { CueButton } from '../../shared/cue-button/cue-button';
import { Fader } from '../../shared/fader/fader';
import { sameSegments, segments } from '../../shared/meter';
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
    <app-cue-button touch [deck]="deck()" />
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
  imports: [ChannelStrip, CueButton, Fader, TPipe],
})
export class PhoneChannel {
  readonly deck = input.required<number>();
  readonly side = input<'a' | 'b'>('a');

  private readonly engine = inject(EngineService);
  protected readonly mixer = inject(MixerService);
  protected readonly marks = MARKS;
  protected readonly letter = computed(() => deckLetter(this.deck()));
  protected readonly volume = computed(() => this.engine.decks()[this.deck()].volume);
  protected readonly level = computed(
    () => segments(this.engine.status()?.meters.decks[this.deck()], METER_SEGMENTS),
    { equal: sameSegments },
  );
}
