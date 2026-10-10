/** Colours a track can be marked with (as in rekordbox), 0xRRGGBB. */
export const TRACK_COLORS: readonly { name: string; value: number }[] = [
  { name: 'Pink', value: 0xff4fd8 },
  { name: 'Red', value: 0xff4d6d },
  { name: 'Orange', value: 0xff8a2e },
  { name: 'Yellow', value: 0xffd23f },
  { name: 'Green', value: 0x34e6a0 },
  { name: 'Aqua', value: 0x2fd6ff },
  { name: 'Blue', value: 0x5b7cff },
  { name: 'Purple', value: 0xa66bff },
];

export function hexColor(value: number): string {
  return '#' + value.toString(16).padStart(6, '0');
}
