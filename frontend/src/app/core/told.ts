/** `before` (none yet: `null`) with `changes`, as the backend tells what changed of a status:
 * each field it sends replaces its own, but `decks`, whose items sent `null` did not change.
 * What did not change keeps its identity, for the views that follow it. */
export function withChanges<T extends { decks: readonly unknown[] }>(
  before: T | null,
  changes: Partial<T>,
): T {
  if (!before) return changes as T;
  const next = { ...before, ...changes };
  const decks = changes.decks as readonly (T['decks'][number] | null)[] | undefined;
  if (decks) next.decks = decks.map((deck, i) => deck ?? before.decks[i]) as T['decks'];
  return next;
}
