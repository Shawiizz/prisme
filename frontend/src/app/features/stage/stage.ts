import { Component, computed, inject } from '@angular/core';
import { EngineService } from '../../core/services/engine.service';
import { MixerService } from '../../core/services/mixer.service';
import { deckLetter } from '../../shared/time';
import { ChannelSlot } from '../mixer/channel-slot/channel-slot';
import { Crossfader } from '../mixer/crossfader/crossfader';
import { DeckCard } from '../deck/deck-card/deck-card';
import { DeckControls } from '../deck/deck-controls/deck-controls';
import { DeckDrop } from '../deck/deck-drop/deck-drop';
import { DeckDropTarget } from '../deck/deck-drop/deck-drop-target';
import { DeckPads } from '../deck/deck-pads/deck-pads';
import { DeckWaveform } from '../deck/deck-waveform/deck-waveform';
import { PhaseStrip } from '../deck/phase-strip/phase-strip';
import { PitchFader } from '../deck/pitch-fader/pitch-fader';

/**
 * The decks in a window of their own, full screen on a screen of their own: the waveforms
 * across it, one over the other around the phase strip, each with its deck's band of figures
 * on one line (over it, but under B's with two decks: the two waveforms together); under them
 * what the hands play, each side's decks at its end and the channels between them. The left
 * side of the crossfader (A, C) above and at the left, the right side (B, D) under and at the
 * right.
 */
@Component({
  selector: 'app-stage',
  template: `
    @for (deck of order(); track deck) {
      @if (deck === 1) {
        <app-phase-strip across />
      }
      <section class="deck" [class.under]="!four() && deck === 1" [appDeckDropTarget]="deck">
        <app-deck-card [deck]="deck" band />
        <app-deck-waveform [deck]="deck" [compact]="four()" />
        <app-deck-drop [deck]="deck" />
      </section>
    }
    <footer class="hands" [class.mixing]="mixer.internal()">
      @for (side of sides(); track $index) {
        <div class="side pr-panel" [class.right]="$index === 1">
          <div class="pitches">
            @for (deck of side; track deck) {
              <app-pitch-fader [deck]="deck" compact />
            }
          </div>
          <div class="decks">
            @for (deck of side; track deck) {
              <div class="deck-hands">
                @if (four()) {
                  <span class="pr-tile letter">{{ letter(deck) }}</span>
                }
                <div class="rail"><app-deck-controls [deck]="deck" /></div>
                <div class="rail pads"><app-deck-pads [deck]="deck" /></div>
              </div>
            }
          </div>
        </div>
        @if ($first && mixer.internal()) {
          <div class="mixer pr-panel">
            <div class="channels">
              @for (deck of order(); track deck) {
                <app-channel-slot [deck]="deck" column [mirror]="right(deck)" [compact]="four()" />
              }
            </div>
            <app-crossfader />
          </div>
        }
      }
    </footer>
  `,
  styles: `
    // the waveforms as tall as the room the hands leave them, up to what mixing asks: past
    // it, the room goes to the pads
    :host {
      display: flex;
      flex-direction: column;
      gap: 6px;
      min-height: 0;
      height: 100%;
      --wave-least: 90px;
      --wave-most: 250px;
    }
    :host(.four) {
      --wave-least: 56px;
      --wave-most: 120px;
    }
    .deck {
      position: relative;
      flex: 100 1 0;
      max-height: calc(var(--wave-most) + 48px);
      display: grid;
      grid-template-rows: auto minmax(var(--wave-least), 1fr);
      gap: 4px;
      // with two decks, B's band under its waveform: the two waveforms together
      &.under {
        grid-template-rows: minmax(var(--wave-least), 1fr) auto;
        app-deck-card {
          order: 1;
        }
      }
      &.dropping {
        outline: 1px solid var(--pr-light);
        outline-offset: 2px;
        border-radius: var(--pr-radius);
      }
    }
    app-deck-card {
      height: auto;
    }
    // the room past the waveforms' most only
    .hands {
      flex: 1 0 auto;
      display: grid;
      grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
      gap: 6px;
      &.mixing {
        grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
      }
    }
    // a deck's side as on a controller, the right one in a mirror: the pitch faders at the
    // outer edge, the transport over the pads
    .side {
      display: flex;
      gap: 10px;
      padding: 10px;
      min-width: 0;
      &.right {
        flex-direction: row-reverse;
      }
    }
    .pitches {
      display: flex;
      gap: 4px;
      app-pitch-fader {
        width: 32px;
      }
    }
    .decks {
      flex: 1;
      min-width: 0;
      display: grid;
      grid-auto-rows: minmax(0, 1fr);
      gap: 10px;
    }
    .deck-hands {
      min-height: 0;
      display: grid;
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: auto minmax(0, 1fr);
      gap: 10px;
    }
    :host(.four) .deck-hands {
      grid-template-columns: auto minmax(0, 1fr);
      gap: 6px 8px;
      .rail {
        grid-column: 2;
      }
    }
    .letter {
      grid-row: span 2;
      align-self: center;
    }
    // the transport and the pads as on a lane's rail, sized by its width
    .rail {
      container: rail / inline-size;
      display: flex;
      align-items: center;
      gap: 5px;
      height: 44px;
      min-width: 0;
      &.pads {
        height: 100%;
      }
    }
    // one mixer: its channels side by side on one panel, the crossfader under them
    .mixer {
      display: grid;
      grid-template-rows: minmax(0, 1fr) auto;
      gap: 10px;
      padding: 10px 14px 8px;
    }
    .channels {
      display: flex;
      justify-content: center;
      gap: 12px;
      min-height: 0;
      app-channel-slot {
        background: none;
        border: none;
      }
    }
  `,
  host: { '[class.four]': 'four()' },
  imports: [
    ChannelSlot,
    Crossfader,
    DeckCard,
    DeckControls,
    DeckDrop,
    DeckDropTarget,
    DeckPads,
    DeckWaveform,
    PhaseStrip,
    PitchFader,
  ],
})
export class Stage {
  private readonly engine = inject(EngineService);
  protected readonly mixer = inject(MixerService);
  protected readonly four = computed(() => this.engine.deckIndexes().length > 2);
  /** The decks from the top: the crossfader's left side over its right. */
  protected readonly order = computed(() => (this.four() ? [2, 0, 1, 3] : [0, 1]));
  /** The decks each side of the hands plays: the left, the right. */
  protected readonly sides = computed(() =>
    this.four()
      ? [
          [2, 0],
          [1, 3],
        ]
      : [[0], [1]],
  );
  protected readonly letter = deckLetter;
  /** A deck of the right side of the crossfader. */
  protected right(deck: number): boolean {
    return deck % 2 === 1;
  }
}
