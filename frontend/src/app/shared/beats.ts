/** The lengths of the loop rolls, in beats: the pads' and the controllers'. */
export const ROLL_BEATS = [1 / 16, 1 / 8, 1 / 4, 1 / 2, 1, 2, 4, 8];

/** The fader cuts, in beats: the pads' and the controllers'. */
export const CUT_BEATS = [1 / 16, 1 / 8, 1 / 4, 1 / 2];

/** 0.25 -> "1/4", 2 -> "2", -0.5 -> "-1/2". */
export function formatBeats(beats: number): string {
  const sign = beats < 0 ? '-' : '';
  const size = Math.abs(beats);
  return size >= 1 ? `${sign}${size}` : `${sign}1/${Math.round(1 / size)}`;
}
