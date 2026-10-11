import { Component, computed, inject } from '@angular/core';
import { EngineService } from '../../core/services/engine.service';
import { MixerService } from '../../core/services/mixer.service';
import { DECK_COUNT } from '../../core/models/engine';
import { deckLetter } from '../../shared/time';
import { Segment, levelSegments } from '../../shared/level';
import { TPipe } from '../../core/i18n/t.pipe';
import { Knob } from '../../shared/knob/knob';
import { Fader } from '../../shared/fader/fader';
import { readouts } from '../../shared/mixer-scales';

/** Segments of the channel meters. */
const METER_SEGMENTS = 16;
/** The headphone volume knob goes up to +3.5 dB. */
const MAX_HEADPHONE_GAIN = 1.5;

/**
 * External mixer: the level sent to each deck's outputs. Software mixer: channel volumes,
 * headphone cues, crossfader and its curve, master limiter, and the headphone knobs (cue/master
 * blend, volume). The channel strips (trim, EQ, filter) sit with their decks (`ChannelStrip`).
 */
@Component({
  selector: 'app-mixer',
  templateUrl: './mixer.html',
  styleUrl: './mixer.scss',
  imports: [Fader, Knob, TPipe],
})
export class Mixer {
  protected readonly engine = inject(EngineService);
  protected readonly mixer = inject(MixerService);
  protected readonly deckIndexes = Array.from({ length: DECK_COUNT }, (_, i) => i);
  protected readonly letter = deckLetter;
  protected readonly MAX_HEADPHONE_GAIN = MAX_HEADPHONE_GAIN;
  protected readonly readouts = readouts;
  protected readonly phonesReadout = (knob: number) => readouts.gain(knob * MAX_HEADPHONE_GAIN);
  protected readonly internal = this.mixer.internal;
  protected readonly masterLevel = computed(() =>
    levelSegments(this.engine.status()?.mainPeak ?? 0, METER_SEGMENTS),
  );

  deckLevel(deck: number): Segment[] {
    return levelSegments(this.engine.status()?.decks[deck]?.peak ?? 0, METER_SEGMENTS);
  }

  /** First and last output of a deck's pair in external mode, counting from 1. */
  outputs(deck: number): string {
    return `${deck * 2 + 1}-${deck * 2 + 2}`;
  }
}
