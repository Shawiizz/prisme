import { t } from '../core/i18n/i18n';

/** Colours a track can be marked with (as in rekordbox), 0xRRGGBB: the tags of tokens.css. */
export const TRACK_COLORS: readonly { name: string; value: number }[] = [
  { name: t('Pink'), value: 0xe86bd6 },
  { name: t('Red'), value: 0xff5442 },
  { name: t('Orange'), value: 0xff9a3c },
  { name: t('Yellow'), value: 0xf5d547 },
  { name: t('Green'), value: 0xa6e25c },
  { name: t('Aqua'), value: 0x3fd3b5 },
  { name: t('Blue'), value: 0x4aa8ff },
  { name: t('Purple'), value: 0x8f7bff },
];

export function hexColor(value: number): string {
  return '#' + value.toString(16).padStart(6, '0');
}
