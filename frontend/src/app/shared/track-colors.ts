import { t } from '../core/i18n/i18n';

/** Colours a track can be marked with (as in rekordbox), 0xRRGGBB. */
export const TRACK_COLORS: readonly { name: string; value: number }[] = [
  { name: t('Pink'), value: 0xff4fd8 },
  { name: t('Red'), value: 0xff4d6d },
  { name: t('Orange'), value: 0xff8a2e },
  { name: t('Yellow'), value: 0xffd23f },
  { name: t('Green'), value: 0x34e6a0 },
  { name: t('Aqua'), value: 0x2fd6ff },
  { name: t('Blue'), value: 0x5b7cff },
  { name: t('Purple'), value: 0xa66bff },
];

export function hexColor(value: number): string {
  return '#' + value.toString(16).padStart(6, '0');
}
