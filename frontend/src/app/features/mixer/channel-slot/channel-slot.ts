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
      <app-channel-strip [deck]="deck()" [trimFirst]="column()" />
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
    // a channel of the mixer under the decks' own window: the trim, the bands from the highs
    // down, the filter and the headphones one over the other, the fader along them as tall as
    // they are, its meter on the side of the other channels (\`mirror\` for a right one)
    :host(.column) {
      display: grid;
      grid-template-columns: auto auto auto;
      grid-template-areas: 'strip fader meter';
      justify-content: center;
      gap: 8px;
      padding: 0;
      .strip {
        grid-area: strip;
        margin: 0;
        align-self: start;
        grid-template-columns: calc(var(--knob-size) + 18px);
        gap: 2px 0;
        --knob-size: var(--pr-channel-knob, 30px);
        --knob-label-width: calc(var(--knob-size) + 16px);
      }
      app-fader {
        grid-area: fader;
      }
      .pr-meter {
        grid-area: meter;
      }
    }
    :host(.column.mirror) {
      grid-template-areas: 'meter fader strip';
    }
    :host(.column.compact) .strip {
      grid-template-columns: 42px;
      gap: 0;
      --knob-size: 24px;
      --knob-label-width: 40px;
    }
  `,
  host: {
    class: 'pr-panel',
    '[class.compact]': 'compact()',
    '[class.column]': 'column()',
    '[class.mirror]': 'mirror()',
  },
  imports: [ChannelStrip, CueButton, Fader, TPipe],
})
export class ChannelSlot {
  readonly deck = input.required<number>();
  /** Half the height, for four decks. */
  readonly compact = input(false, { transform: booleanAttribute });
  /** Its knobs one over the other as on a mixer, its fader along them, for a channel beside
   * the others. */
  readonly column = input(false, { transform: booleanAttribute });
  /** A right channel of them: its meter at the left of its fader. */
  readonly mirror = input(false, { transform: booleanAttribute });

  private readonly engine = inject(EngineService);
  protected readonly mixer = inject(MixerService);
  protected readonly letter = computed(() => deckLetter(this.deck()));
  protected readonly volume = computed(() => this.engine.decks()[this.deck()].volume);
  protected readonly level = meterSegments(
    () => this.engine.status()?.meters.decks[this.deck()],
    METER_SEGMENTS,
  );
}
