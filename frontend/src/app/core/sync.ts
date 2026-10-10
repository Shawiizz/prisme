import { DeckStatus } from './models/engine';

/** Power of two bringing `bpm` closest to `otherBpm`: 70 BPM goes with 140. */
export function octaveRatio(bpm: number, otherBpm: number): number {
  return 2 ** Math.round(Math.log2(bpm / otherBpm));
}

/** How far a deck's beats are from another deck's, in beats within [-0.5, 0.5]; positive = ahead. */
export function phaseOffset(deck: DeckStatus, other: DeckStatus): number | null {
  if (deck.bpm === null || other.bpm === null || deck.beatPhase === null || other.beatPhase === null) {
    return null;
  }
  const otherPhase = (other.beatPhase * octaveRatio(deck.bpm, other.bpm)) % 1;
  const gap = deck.beatPhase - otherPhase;
  return gap - Math.round(gap);
}

/** Relative speed change that would give a deck the other deck's tempo (0.008 = +0.8 %). */
export function pitchToMatch(deck: DeckStatus, other: DeckStatus): number | null {
  if (!deck.bpm || !other.bpm) return null;
  return (octaveRatio(deck.bpm, other.bpm) * other.bpm) / deck.bpm - 1;
}
