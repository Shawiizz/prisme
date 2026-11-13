import { Signal, computed, inject } from '@angular/core';
import { EngineService } from './services/engine.service';
import { decimal, formatPitch, formatRemaining } from '../shared/time';

/** The remaining time turns amber, then red and blinking, near the end of the track. */
const WARN_S = 30;
const ALERT_S = 10;

/**
 * What a deck tells from afar, as its card and its strip on a phone share it: the time
 * left, the tempo heard and the key heard. To be called where `inject` works.
 */
export function deckReadouts(deck: Signal<number>) {
  const engine = inject(EngineService);
  const track = computed(() => engine.tracks()[deck()]);
  const status = computed(() => engine.status()?.decks[deck()] ?? null);
  const remainingS = computed(() => {
    const s = status();
    return s?.loaded ? Math.max(0, s.durationS - s.positionS) : null;
  });
  const remaining = computed(() => {
    const s = remainingS();
    return s === null ? '—' : formatRemaining(s);
  });
  const ending = computed(() => {
    const s = remainingS();
    if (s === null || !status()?.playing) return null;
    return s < ALERT_S ? 'alert' : s < WARN_S ? 'warn' : null;
  });
  const rate = computed(() => engine.playbackRate(deck()));
  const bpm = computed(() => {
    const tempo = track()?.tempo;
    return tempo ? decimal(tempo.bpm * rate(), 2) : null;
  });
  /** The pitch applied to the track's own tempo. */
  const pitch = computed(() => (track()?.tempo ? formatPitch(rate()) : ''));
  /** The key heard: shifted by the pitch unless the key lock holds it. */
  const key = computed(() => engine.heardKey(deck()));
  return { track, status, remaining, ending, bpm, pitch, key };
}
