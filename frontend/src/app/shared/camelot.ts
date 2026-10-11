import { LANGUAGE, t } from '../core/i18n/i18n';

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

/**
 * The smallest transposition (-5..+6 semitones) after which `code` mixes harmonically
 * with `target` (see `camelotCompatible`); landing on the target's own wheel number wins
 * a tie.
 */
export function semitonesToMix(code: string, target: string): number | null {
  const c = parse(code);
  const t = parse(target);
  if (!c || !t) return null;
  const steps = t.number - c.number;
  const candidates = c.minor === t.minor ? [steps, steps - 1, steps + 1] : [steps];
  return candidates
    .map(semitonesForSteps)
    .reduce((best, s) => (Math.abs(s) < Math.abs(best) ? s : best));
}

/** A semitone is 7 steps around the wheel, and 7 × 7 = 49 ≡ 1 (mod 12). */
function semitonesForSteps(steps: number): number {
  const semitones = (((7 * steps) % 12) + 12) % 12;
  return semitones > 6 ? semitones - 12 : semitones;
}
/** Sort rank: 1A, 1B, 2A, 2B... */
export function camelotRank(code: string | undefined | null): number {
  const c = code ? parse(code) : null;
  return c ? c.number * 2 + (c.minor ? 0 : 1) : Number.MAX_SAFE_INTEGER;
}

/** Hue of a key chip (`--h` of `.pr-key`): one per wheel number, see tokens.css. */
export function camelotHue(code: string): string | null {
  const c = parse(code);
  return c ? `var(--pr-key-hue-${c.number})` : null;
}

export function camelotMajor(code: string): boolean {
  return parse(code)?.minor === false;
}

const NOTES: Record<string, string> = {
  C: 'Do',
  D: 'Ré',
  E: 'Mi',
  F: 'Fa',
  G: 'Sol',
  A: 'La',
  B: 'Si',
};

/** "5A" -> "C minor", "Do mineur" in French; accidentals as ♯ and ♭. */
export function keyName(code: string): string {
  const c = parse(code);
  if (!c) return '';
  const name = (c.minor ? MINOR_NAMES : MAJOR_NAMES)[c.number].replace(/m$/, '');
  const note = LANGUAGE === 'fr' ? NOTES[name[0]] : name[0];
  const accidental = name.slice(1).replace('#', '♯').replace('b', '♭');
  return `${note}${accidental} ${c.minor ? t('minor') : t('major')}`;
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
