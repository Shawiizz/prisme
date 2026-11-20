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

/** "C♯ minor", "Do♯ mineur" in French. */
export function musicalKeyName(key: MusicalKey): string {
  const note = language() === 'fr' ? NOTES[key.note] : key.note;
  return `${note}${key.accidental} ${key.minor ? t('minor') : t('major')}`;
}
