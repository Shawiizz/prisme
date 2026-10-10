import { Component, computed, inject, input } from '@angular/core';
import { EngineService } from '../../../core/services/engine.service';
import { deckColor, formatPitch } from '../../../shared/time';

/** Pitch range of the tempo fader (±8%). */
const PITCH_RANGE = 0.08;

/** Transport (cue, play) and tempo fader, used when vinyl control is off. */
@Component({
  selector: 'app-deck-controls',
  templateUrl: './deck-controls.html',
  styleUrl: './deck-controls.scss',
})
export class DeckControls {
  readonly deck = input.required<number>();

  protected readonly engine = inject(EngineService);
  protected readonly color = computed(() => deckColor(this.deck()));
  protected readonly status = computed(() => this.engine.status()?.decks[this.deck()] ?? null);
  protected readonly settings = computed(() => this.engine.decks()[this.deck()]);
  protected readonly vinyl = computed(() => this.settings().vinylEnabled);
  protected readonly pitchText = computed(() => formatPitch(this.settings().pitch));
  protected readonly range = PITCH_RANGE;

  togglePlay() {
    const s = this.status();
    if (s?.loaded) this.engine.setPlaying(this.deck(), !s.playing);
  }

  cue() {
    this.engine.cue(this.deck());
  }

  onPitch(event: Event) {
    const v = Number((event.target as HTMLInputElement).value);
    this.engine.setPitch(this.deck(), 1 + v);
  }

  resetPitch() {
    this.engine.setPitch(this.deck(), 1);
  }
}
