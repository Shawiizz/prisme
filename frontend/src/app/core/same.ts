/** Whether `a` and `b` hold the same values (plain data, as the backend sends it). */
export function same<T>(a: T, b: T): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}
