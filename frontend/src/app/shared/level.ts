import { Signal, computed, linkedSignal } from '@angular/core';

/** A segment of a level meter (`.pr-meter`): off, lit, hot or clipping. */
export type Segment = '' | 'is-lit' | 'is-hot' | 'is-clip';

/** Range of a meter: its lowest segment lights at this level (dBFS). */
const FLOOR_DB = -48;
/** A meter rises at once and falls back this fast (dB per second), as a mixer's does. */
const RELEASE_DB_S = 20;
/** The highest level stays lit this long (ms) before it falls back too. */
const HOLD_MS = 1000;

function toDb(peak: number): number {
  return peak > 0 ? 20 * Math.log10(peak) : -Infinity;
}

function segments(db: number, held: number, count: number): Segment[] {
  return Array.from({ length: count }, (_, i) => {
    const at = FLOOR_DB * (1 - i / (count - 1));
    const next = i === count - 1 ? Infinity : FLOOR_DB * (1 - (i + 1) / (count - 1));
    if (db < at && !(held >= at && held < next)) return '';
    return i === count - 1 ? 'is-clip' : at >= -6 ? 'is-hot' : 'is-lit';
  });
}

/**
 * The `count` segments of a meter for a peak (1 = full scale), lowest first: grey up to
 * -6 dB, amber up to 0, the last one red at full scale.
 */
export function levelSegments(peak: number, count: number): Segment[] {
  const db = toDb(peak);
  return segments(db, db, count);
}

interface Ballistics {
  db: number;
  held: number;
  heldAt: number;
  at: number;
}

/**
 * A meter of the peaks the engine reports, read as a mixer's meter reads: it rises at once
 * and falls back slowly, its highest level held a moment above it. `peak` is read where
 * the status of the engine is, so that the meter falls back between two peaks too.
 */
export function levelMeter(peak: () => number, count: number): Signal<Segment[]> {
  const state = linkedSignal<number[], Ballistics>({
    source: () => [peak()],
    computation: ([value], previous) => {
      const now = performance.now();
      const db = toDb(value);
      const last = previous?.value;
      if (!last) return { db, held: db, heldAt: now, at: now };
      const fall = (RELEASE_DB_S * (now - last.at)) / 1000;
      const shown = Math.max(db, last.db - fall);
      if (shown >= last.held) return { db: shown, held: shown, heldAt: now, at: now };
      const held = now - last.heldAt > HOLD_MS ? Math.max(shown, last.held - fall) : last.held;
      return { db: shown, held, heldAt: last.heldAt, at: now };
    },
  });
  return computed(() => segments(state().db, state().held, count));
}
