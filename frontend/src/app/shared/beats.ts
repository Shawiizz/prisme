/** 0.25 -> "1/4", 2 -> "2", -0.5 -> "-1/2". */
export function formatBeats(beats: number): string {
  const sign = beats < 0 ? '-' : '';
  const size = Math.abs(beats);
  return size >= 1 ? `${sign}${size}` : `${sign}1/${Math.round(1 / size)}`;
}

/** Length of a loop in beats, if it is a whole power of two (1/32 to 64) within 1 %. */
export function loopBeats(lengthS: number, beatPeriodS: number): number | null {
  const beats = lengthS / beatPeriodS;
  const power = 2 ** Math.round(Math.log2(beats));
  return power >= 1 / 32 && power <= 64 && Math.abs(beats / power - 1) < 0.01 ? power : null;
}
