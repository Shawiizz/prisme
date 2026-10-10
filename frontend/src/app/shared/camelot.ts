/** Camelot wheel helpers ("8A" = A minor, "8B" = C major). */

interface Camelot {
  number: number;
  minor: boolean;
}

function parse(code: string): Camelot | null {
  const m = /^(\d{1,2})([AB])$/.exec(code);
  return m ? { number: +m[1], minor: m[2] === 'A' } : null;
}

/** Key names by Camelot number (index 0 unused). */
const MINOR_NAMES = [
  '',
  'G#m',
  'Ebm',
  'Bbm',
  'Fm',
  'Cm',
  'Gm',
  'Dm',
  'Am',
  'Em',
  'Bm',
  'F#m',
  'C#m',
];
const MAJOR_NAMES = ['', 'B', 'F#', 'Db', 'Ab', 'Eb', 'Bb', 'F', 'C', 'G', 'D', 'A', 'E'];

/**
 * The key `semitones` higher (lower if negative): a semitone is 7 steps around the
 * wheel.
 */
export function transposeCamelot(
  code: string,
  semitones: number,
): { camelot: string; name: string } | null {
  const c = parse(code);
  if (!c) return null;
  const steps = (((7 * semitones) % 12) + 12) % 12;
  const number = ((c.number - 1 + steps) % 12) + 1;
  return {
    camelot: `${number}${c.minor ? 'A' : 'B'}`,
    name: (c.minor ? MINOR_NAMES : MAJOR_NAMES)[number],
  };
}

/** Sort rank: 1A, 1B, 2A, 2B... */
export function camelotRank(code: string | undefined | null): number {
  const c = code ? parse(code) : null;
  return c ? c.number * 2 + (c.minor ? 0 : 1) : Number.MAX_SAFE_INTEGER;
}

/** One colour per wheel position, minor keys slightly darker. */
export function camelotColor(code: string): string {
  const c = parse(code);
  if (!c) return 'var(--muted)';
  const hue = (c.number - 1) * 30 + 165;
  return `hsl(${hue % 360} 75% ${c.minor ? 62 : 72}%)`;
}

/**
 * Keys that mix harmonically: the same key, one step around the wheel, or the
 * relative major / minor.
 */
export function camelotCompatible(a: string, b: string): boolean {
  const x = parse(a);
  const y = parse(b);
  if (!x || !y) return false;
  if (x.minor === y.minor) {
    const d = Math.abs(x.number - y.number);
    return d === 0 || d === 1 || d === 11;
  }
  return x.number === y.number;
}
