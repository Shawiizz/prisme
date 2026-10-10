import { Component, computed, inject } from '@angular/core';
import { EngineService } from '../../core/services/engine.service';
import { MixerService } from '../../core/services/mixer.service';
import { DECK_COUNT } from '../../core/models/engine';
import { deckColor } from '../../shared/time';
import { TPipe } from '../../core/i18n/t.pipe';

/**
 * External mixer: the level sent to each deck's outputs. Software mixer: channel volumes,
 * headphone cues, crossfader and its curve, master limiter, cue/master blend and headphone
 * volume. The channel strips (trim, EQ, filter) sit with their decks (`ChannelStrip`).
 */
@Component({
  selector: 'app-mixer',
  templateUrl: './mixer.html',
  styleUrl: './mixer.scss',
  imports: [TPipe],
})
export class Mixer {
  protected readonly engine = inject(EngineService);
  protected readonly mixer = inject(MixerService);
  protected readonly deckIndexes = Array.from({ length: DECK_COUNT }, (_, i) => i);
  protected readonly color = deckColor;
  protected readonly internal = this.mixer.internal;
  /** External mode asked for, but the device has too few outputs. */
  protected readonly fallback = computed(
    () => this.mixer.mode() === 'external' && this.mixer.modeInUse() === 'internal',
  );
  protected readonly peak = computed(() => this.engine.status()?.mainPeak ?? 0);

  deckPeak(deck: number): number {
    return this.engine.status()?.decks[deck]?.peak ?? 0;
  }

  /** First and last output of a deck's pair in external mode, counting from 1. */
  outputs(deck: number): string {
    return `${deck * 2 + 1}-${deck * 2 + 2}`;
  }

  onVolume(deck: number, event: Event) {
    this.mixer.setVolume(deck, Number((event.target as HTMLInputElement).value));
  }

  onCrossfader(event: Event) {
    this.mixer.setCrossfader(Number((event.target as HTMLInputElement).value));
  }

  onCueMix(event: Event) {
    this.mixer.setCueMix(Number((event.target as HTMLInputElement).value));
  }

  onHeadphoneGain(event: Event) {
    this.mixer.setHeadphoneGain(Number((event.target as HTMLInputElement).value));
  }
}
