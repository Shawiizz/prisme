import { Told } from './models/engine';

/** `before` (none yet: `null`) with `changes`, as the backend tells what changed of a status:
 * each field it sends replaces its own, but `decks`, whose items sent `null` did not change.
 * What did not change keeps its identity, for the views that follow it. */
export function withChanges<T extends { decks: readonly unknown[] }>(
  before: T | null,
  changes: Told<T>,
): T {
  // the first status is told whole
  if (!before) return changes as unknown as T;
  const { decks, ...rest } = changes;
  const next = { ...before, ...rest } as T;
  if (decks) next.decks = decks.map((deck, i) => deck ?? before.decks[i]) as T['decks'];
  return next;
}
