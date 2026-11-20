/** Power of two bringing `bpm` closest to `otherBpm`: 70 BPM goes with 140. */
export function octaveRatio(bpm: number, otherBpm: number): number {
  return 2 ** Math.round(Math.log2(bpm / otherBpm));
}
