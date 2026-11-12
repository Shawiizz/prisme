import { Component, booleanAttribute, inject, input } from '@angular/core';
import { MixerService } from '../../../core/services/mixer.service';
import { ChannelSlot } from '../../mixer/channel-slot/channel-slot';
import { DeckCard } from '../deck-card/deck-card';
import { DeckControls } from '../deck-controls/deck-controls';
import { DeckPads } from '../deck-pads/deck-pads';
import { DeckWaveform } from '../deck-waveform/deck-waveform';
import { PitchFader } from '../pitch-fader/pitch-fader';

/**
 * Everything about a deck in one band, read from left to right: its panel, its waveform,
 * its channel when the software mixer runs, its pitch fader at the end where a turntable
 * has it; below, a rail with what plays it and its pads.
 */
@Component({
  selector: 'app-deck-lane',
  template: `
    <app-deck-card [deck]="deck()" />
    <app-deck-waveform [deck]="deck()" />
    @if (mixer.internal()) {
      <app-channel-slot [deck]="deck()" />
    }
    <app-pitch-fader [deck]="deck()" />
    <div class="rail pr-panel">
      <app-deck-controls [deck]="deck()" />
      <i class="divider"></i>
      <app-deck-pads [deck]="deck()" />
    </div>
  `,
  styles: `
    :host {
      display: grid;
      grid-template-columns: var(--pr-lane-columns);
      grid-template-rows: 124px 44px;
      gap: 4px 10px;
      min-width: 0;
    }
    app-pitch-fader {
      grid-column: -2;
      grid-row: 1 / span 2;
    }
    // four decks: half the height, the channel of the software mixer over both rows
    :host(.compact) {
      grid-template-rows: 44px 44px;
      app-channel-slot {
        grid-column: 3;
        grid-row: 1 / span 2;
      }
      .rail {
        grid-column: 1 / 3;
      }
    }
    .rail {
      grid-column: 1 / -2;
      grid-row: 2;
      container: rail / inline-size;
      display: flex;
      align-items: center;
      gap: 5px;
      padding: 0 4px;
      min-width: 0;
    }
    .divider {
      align-self: stretch;
      width: 1px;
      margin: 6px 3px;
      background: var(--pr-line);
    }
  `,
  host: { '[class.compact]': 'compact()' },
  imports: [ChannelSlot, DeckCard, DeckControls, DeckPads, DeckWaveform, PitchFader],
})
export class DeckLane {
  readonly deck = input.required<number>();
  /** Half the height, for four decks. */
  readonly compact = input(false, { transform: booleanAttribute });
  protected readonly mixer = inject(MixerService);
}
