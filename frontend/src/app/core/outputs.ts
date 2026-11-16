/** A pair of outputs the booth can be on: heard there, or `taken` by the headphones. */
export interface BoothPair {
  first: number;
  taken: boolean;
}

/**
 * The pairs the booth can take on a device with `outputs` (0-based first channels): each after
 * the master's 1-2 but for the headphones' (`headphones`, their first, if they have one), and
 * the one `chosen` if it is not among them, marked taken when the headphones have it.
 */
export function boothPairs(
  outputs: number,
  headphones: number | null,
  chosen: number | null,
): BoothPair[] {
  const pairs: BoothPair[] = [];
  for (let first = 2; first + 2 <= outputs; first += 2)
    if (first !== headphones || first === chosen)
      pairs.push({ first, taken: first === headphones });
  if (chosen !== null && !pairs.some((p) => p.first === chosen))
    pairs.push({ first: chosen, taken: false });
  return pairs.sort((a, b) => a.first - b.first);
}
