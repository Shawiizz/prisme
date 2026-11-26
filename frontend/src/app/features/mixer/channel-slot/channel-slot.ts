import { Component, booleanAttribute, computed, inject, input } from '@angular/core';
import { EngineService } from '../../../core/services/engine.service';
import { MixerService } from '../../../core/services/mixer.service';
import { TPipe } from '../../../core/i18n/t.pipe';
import { CueButton } from '../../../shared/cue-button/cue-button';
import { Fader } from '../../../shared/fader/fader';
import { meterSegments } from '../../../shared/meter';
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
      <app-cue-button [deck]="deck()" />
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
    app-cue-button {
      width: 28px;
      height: 28px;
    }
    :host(.compact) {
      padding-block: 4px;
      .strip {
        row-gap: 0;
      }
    }
    .pr-meter {
      width: 5px;
    }
    app-fader {
      height: auto;
    }
    // beside the other channels, under the decks' own window: two knobs a row, smaller with
    // four channels
    :host(.narrow) .strip {
      grid-template-columns: repeat(2, 44px);
    }
    :host(.narrow.compact) .strip {
      grid-template-columns: repeat(2, 36px);
      --knob-size: 24px;
      --knob-label-width: 34px;
    }
  `,
  host: { class: 'pr-panel', '[class.compact]': 'compact()', '[class.narrow]': 'narrow()' },
  imports: [ChannelStrip, CueButton, Fader, TPipe],
})
export class ChannelSlot {
  readonly deck = input.required<number>();
  /** Half the height, for four decks. */
  readonly compact = input(false, { transform: booleanAttribute });
  /** Its knobs two a row, for a channel beside the others. */
  readonly narrow = input(false, { transform: booleanAttribute });

  private readonly engine = inject(EngineService);
  protected readonly mixer = inject(MixerService);
  protected readonly letter = computed(() => deckLetter(this.deck()));
  protected readonly volume = computed(() => this.engine.decks()[this.deck()].volume);
  protected readonly level = meterSegments(
    () => this.engine.status()?.meters.decks[this.deck()],
    METER_SEGMENTS,
  );
}
