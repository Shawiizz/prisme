import { Signal, computed, inject } from '@angular/core';
import { EngineService } from './services/engine.service';
import { decimal, formatPitch, formatRemaining } from '../shared/time';

/**
 * What a deck tells from afar, as its card, its strip on a phone and its line of what plays
 * share it: the time left, the tempo heard and the key heard. To be called where `inject`
 * works.
 */
export function deckReadouts(deck: Signal<number>) {
  const engine = inject(EngineService);
  const track = computed(() => engine.tracks()[deck()]);
  const status = computed(() => engine.status()?.decks[deck()] ?? null);
  const remaining = computed(() => {
    const s = status()?.remainingS ?? null;
    return s === null ? '—' : formatRemaining(s);
  });
  /** The remaining time turns amber near the end of the track, then red and blinking. */
  const ending = computed(() => {
    const ending = status()?.ending;
    return ending === 'soon' ? 'alert' : ending === 'on' ? 'warn' : null;
  });
  const bpm = computed(() => {
    const bpm = status()?.bpm;
    return bpm == null ? null : decimal(bpm, 2);
  });
  /** The pitch applied to the track's own tempo. */
  const pitch = computed(() => {
    const rate = status()?.rate;
    return track()?.tempo && rate !== undefined ? formatPitch(rate) : '';
  });
  /** The key heard: shifted by the pitch unless the key lock holds it. */
  const key = computed(() => status()?.heardKey ?? null);
  return { track, status, remaining, ending, bpm, pitch, key };
}
