/** A yes or no the DJ chose, kept in the page's storage from one start to the next; `null`
 * if never chosen (or the storage cannot be read). */
export function storedChoice(key: string): boolean | null {
  try {
    const saved = localStorage.getItem(key);
    return saved === 'true' ? true : saved === 'false' ? false : null;
  } catch {
    return null;
  }
}

export function storeChoice(key: string, value: boolean) {
  try {
    localStorage.setItem(key, String(value));
  } catch {
    // kept for this run only
  }
}
