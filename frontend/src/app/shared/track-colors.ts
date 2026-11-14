import { t } from '../core/i18n/i18n';

/** Colours a track can be marked with (as in rekordbox), 0xRRGGBB: the tags of tokens.css. */
export const TRACK_COLORS: readonly { name: string; value: number }[] = [
  {
    get name() {
      return t('Pink');
    },
    value: 0xe86bd6,
  },
  {
    get name() {
      return t('Red');
    },
    value: 0xff5442,
  },
  {
    get name() {
      return t('Orange');
    },
    value: 0xff9a3c,
  },
  {
    get name() {
      return t('Yellow');
    },
    value: 0xf5d547,
  },
  {
    get name() {
      return t('Green');
    },
    value: 0xa6e25c,
  },
  {
    get name() {
      return t('Aqua');
    },
    value: 0x3fd3b5,
  },
  {
    get name() {
      return t('Blue');
    },
    value: 0x4aa8ff,
  },
  {
    get name() {
      return t('Purple');
    },
    value: 0x8f7bff,
  },
];

export function hexColor(value: number): string {
  return '#' + value.toString(16).padStart(6, '0');
}
