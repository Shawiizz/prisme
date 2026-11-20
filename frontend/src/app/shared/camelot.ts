import { language, t } from '../core/i18n/i18n';

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
  const note = language() === 'fr' ? NOTES[name[0]] : name[0];
  const accidental = name.slice(1).replace('#', '♯').replace('b', '♭');
  return `${note}${accidental} ${c.minor ? t('minor') : t('major')}`;
}
