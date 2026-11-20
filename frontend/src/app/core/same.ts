/** Whether `a` and `b` hold the same values (plain data, as the backend sends it). */
export function same<T>(a: T, b: T): boolean {
  return a === b || JSON.stringify(a) === JSON.stringify(b);
}

/** Whether `a` and `b` hold the same items, one by one: what a list of plain values computed
 * again from a new state is compared by, its views drawn again only when one changed. */
export function sameItems<T>(a: readonly T[], b: readonly T[]): boolean {
  return a.length === b.length && a.every((item, i) => Object.is(item, b[i]));
}
