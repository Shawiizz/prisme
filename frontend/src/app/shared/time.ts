import { computed } from '@angular/core';
import { locale } from '../core/i18n/i18n';

const separator = computed(() => (1.5).toLocaleString(locale()).charAt(1));

/** 126.004, 2 -> "126.00" ("126,00" in French). */
export function decimal(value: number, digits: number): string {
  return value.toFixed(digits).replace('.', separator());
}

/** 83.4 -> "1:23.4" */
export function formatTime(seconds: number): string {
  // rounded to the tenth before the minutes are counted: 59.97 reads "1:00.0", not "0:60.0"
  const tenths = Math.round(Math.max(0, seconds) * 10);
  const m = Math.floor(tenths / 600);
  return `${m}:${decimal((tenths - m * 600) / 10, 1).padStart(4, '0')}`;
}

/** Remaining time with a true minus sign: "−2:15.7". */
export function formatRemaining(seconds: number): string {
  return '−' + formatTime(seconds);
}

/** 1.034 -> "+3.40 %", to `digits` decimals. */
export function formatPitch(ratio: number, digits = 2): string {
  // round first so 0.99996 reads "+0.00 %", not "−0.00 %"
  const scale = 10 ** digits;
  const pct = Math.round((ratio - 1) * 100 * scale) / scale || 0;
  return `${pct >= 0 ? '+' : '−'}${decimal(Math.abs(pct), digits)} %`;
}

/** -1.068 -> "−1.068×" (1 = normal speed, negative = backwards). */
export function formatSpeed(speed: number): string {
  return `${speed >= 0 ? '+' : '−'}${decimal(Math.abs(speed), 3)}×`;
}

/** Decks are named by letter: A, B. */
export function deckLetter(deck: number): string {
  return String.fromCharCode(65 + deck);
}

/** How long something has run: `1:02:05` or `12:34`. */
export function formatClock(seconds: number): string {
  const s = Math.floor(seconds);
  const mmss = `${String(Math.floor(s / 60) % 60).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;
  return s >= 3600 ? `${Math.floor(s / 3600)}:${mmss}` : mmss;
}
