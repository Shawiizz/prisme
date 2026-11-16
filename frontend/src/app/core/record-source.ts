import { RecordSource } from './models/engine';

export function sameSource(a: RecordSource, b: RecordSource): boolean {
  return a.kind === 'master' ? b.kind === 'master' : b.kind === 'input' && a.first === b.first;
}

/**
 * What a recording started without asking takes (from a controller, the command palette): the
 * source recorded `last` if it is still `offered` and, for inputs, no turntable comes in there
 * (they are among the `free` ones), else the software mixer's master if it is offered; none
 * otherwise, rather than inputs that may be silent or carry a record's timecode.
 */
export function preferredSource(
  offered: RecordSource[],
  last: RecordSource | null,
  free: number[],
): RecordSource | null {
  const kept =
    last &&
    offered.some((s) => sameSource(s, last)) &&
    (last.kind === 'master' || free.includes(last.first));
  return kept ? last : (offered.find((s) => s.kind === 'master') ?? null);
}
