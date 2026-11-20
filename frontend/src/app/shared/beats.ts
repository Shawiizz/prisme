/** 0.25 -> "1/4", 2 -> "2", -0.5 -> "-1/2". */
export function formatBeats(beats: number): string {
  const sign = beats < 0 ? '-' : '';
  const size = Math.abs(beats);
  return size >= 1 ? `${sign}${size}` : `${sign}1/${Math.round(1 / size)}`;
}
