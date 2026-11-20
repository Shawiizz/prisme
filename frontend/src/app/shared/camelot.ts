import { language, t } from '../core/i18n/i18n';
import { MusicalKey } from '../core/models/engine';

/** Hue of a key chip (`--h` of `.pr-key`): one per number of the Camelot wheel, see
 * tokens.css. */
export function keyHue(key: MusicalKey): string {
  return `var(--pr-key-hue-${key.number})`;
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

/** "Cm" -> "C minor", "Do mineur" in French; accidentals as ♯ and ♭. */
export function musicalKeyName(key: MusicalKey): string {
  const name = key.minor ? key.name.slice(0, -1) : key.name;
  const note = language() === 'fr' ? NOTES[name[0]] : name[0];
  const accidental = name.slice(1).replace('#', '♯').replace('b', '♭');
  return `${note}${accidental} ${key.minor ? t('minor') : t('major')}`;
}
