/** 83.4 -> "1:23.4" */
export function formatTime(seconds: number): string {
  const s = Math.max(0, seconds);
  const m = Math.floor(s / 60);
  const rest = s - m * 60;
  return `${m}:${rest.toFixed(1).padStart(4, '0')}`;
}

/** 1.034 -> "+3.40%" */
export function formatPitch(ratio: number): string {
  // round first so 0.99996 reads "+0.00%", not "-0.00%"
  const pct = Math.round((ratio - 1) * 10000) / 100 || 0;
  return `${pct >= 0 ? '+' : ''}${pct.toFixed(2)}%`;
}

/** Colour of a deck (CSS variable). */
export function deckColor(deck: number): string {
  return deck % 2 === 0 ? 'var(--deck-1)' : 'var(--deck-2)';
}
