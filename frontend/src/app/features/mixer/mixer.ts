import { Component, computed, inject } from '@angular/core';
import { EngineService } from '../../core/services/engine.service';
import { DECK_COUNT } from '../../core/models/engine';
import { deckColor } from '../../shared/time';

/** Channel volumes and crossfader. */
@Component({
  selector: 'app-mixer',
  templateUrl: './mixer.html',
  styleUrl: './mixer.scss',
})
export class Mixer {
  protected readonly engine = inject(EngineService);
  protected readonly deckIndexes = Array.from({ length: DECK_COUNT }, (_, i) => i);
  protected readonly color = deckColor;
  protected readonly peak = computed(() => this.engine.status()?.mainPeak ?? 0);

  onVolume(deck: number, event: Event) {
    this.engine.setVolume(deck, Number((event.target as HTMLInputElement).value));
  }

  onCrossfader(event: Event) {
    this.engine.setCrossfader(Number((event.target as HTMLInputElement).value));
  }
}
