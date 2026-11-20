import type { StemPart } from './models/status';

/** A part of a separated track that a deck mutes or turns down: one stem, or the
 * instrumental (everything but the vocals). */
export type { StemPart };

/** The name of `part` in the actions' ids: `melody` for the other instruments. */
export function partId(part: StemPart): string {
  return part === 'other' ? 'melody' : part;
}
