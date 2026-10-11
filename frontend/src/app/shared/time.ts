import { LOCALE } from '../core/i18n/i18n';

const separator = (1.5).toLocaleString(LOCALE).charAt(1);

/** 126.004, 2 -> "126.00" ("126,00" in French). */
export function decimal(value: number, digits: number): string {
  return value.toFixed(digits).replace('.', separator);
}

/** 83.4 -> "1:23.4" */
export function formatTime(seconds: number): string {
  const s = Math.max(0, seconds);
  const m = Math.floor(s / 60);
  return `${m}:${decimal(s - m * 60, 1).padStart(4, '0')}`;
}

/** Remaining time with a true minus sign: "−2:15.7". */
export function formatRemaining(seconds: number): string {
  return '−' + formatTime(seconds);
}

/** 1.034 -> "+3.40 %" */
export function formatPitch(ratio: number): string {
  // round first so 0.99996 reads "+0.00 %", not "−0.00 %"
  const pct = Math.round((ratio - 1) * 10000) / 100 || 0;
  return `${pct >= 0 ? '+' : '−'}${decimal(Math.abs(pct), 2)} %`;
}

/** -1.068 -> "−1.068×" (1 = normal speed, negative = backwards). */
export function formatSpeed(speed: number): string {
  return `${speed >= 0 ? '+' : '−'}${decimal(Math.abs(speed), 3)}×`;
}

/** Decks are named by letter: A, B. */
export function deckLetter(deck: number): string {
  return String.fromCharCode(65 + deck);
}
