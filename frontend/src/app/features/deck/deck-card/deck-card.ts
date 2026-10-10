import { Component, computed, inject, input } from '@angular/core';
import { EngineService } from '../../../core/services/engine.service';
import { deckColor, formatPitch, formatTime } from '../../../shared/time';

/**
 * Deck header: number tile in the deck colour, title and time, labelled stats and the
 * vinyl (DVS) controls.
 */
@Component({
  selector: 'app-deck-card',
  templateUrl: './deck-card.html',
  styleUrl: './deck-card.scss',
})
export class DeckCard {
  readonly deck = input.required<number>();

  protected readonly engine = inject(EngineService);
  protected readonly color = computed(() => deckColor(this.deck()));
  protected readonly track = computed(() => this.engine.tracks()[this.deck()]);
  protected readonly status = computed(() => this.engine.status()?.decks[this.deck()] ?? null);
  protected readonly settings = computed(() => this.engine.decks()[this.deck()]);
  protected readonly loading = computed(() => this.engine.loading()[this.deck()]);

  protected readonly elapsed = computed(() => formatTime(this.status()?.positionS ?? 0));
  protected readonly remaining = computed(() => {
    const s = this.status();
    return s ? '-' + formatTime(s.durationS - s.positionS) : '';
  });
  /** Vinyl: the platter speed (while it turns). Internal: the tempo fader. */
  protected readonly pitch = computed(() => {
    const speed = Math.abs(this.status()?.speed ?? 0);
    const live = this.settings().vinylEnabled && speed > 0.05;
    return formatPitch(live ? speed : this.settings().pitch);
  });
  protected readonly signal = computed(() => this.status()?.vinyl.signal ?? 'none');

  toggleVinyl() {
    this.engine.setVinyl(this.deck(), { enabled: !this.settings().vinylEnabled });
  }

  toggleMode() {
    this.engine.setVinyl(this.deck(), { mode: this.settings().vinylMode === 'absolute' ? 'relative' : 'absolute' });
  }
}
