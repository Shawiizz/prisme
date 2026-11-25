import type { GridAnchor, Tempo } from './models/backend';

/** Where a stretch of a grid starts and its beats a second: the stretch before anchor `after`
 * (the first stretch before the first anchor, the tempo after the last past it), as the
 * backend counts them. */
function stretch(tempo: Tempo, after: number): [GridAnchor, number] {
  const anchors = tempo.anchors;
  if (anchors.length === 0) return [{ beat: 0, timeS: tempo.firstBeatS }, tempo.bpm / 60];
  if (after >= anchors.length) return [anchors[anchors.length - 1], tempo.bpmAfter / 60];
  const i = Math.min(Math.max(after - 1, 0), anchors.length - 2);
  const [from, to] = [anchors[i], anchors[i + 1]];
  return [from, (to.beat - from.beat) / (to.timeS - from.timeS)];
}

/** The stretch `timeS` falls in. */
function stretchAtTime(tempo: Tempo, timeS: number): [GridAnchor, number] {
  let after = 0;
  while (after < tempo.anchors.length && tempo.anchors[after].timeS <= timeS) after++;
  return stretch(tempo, after);
}

/** The stretch beat `beats` falls in. */
function stretchAtBeat(tempo: Tempo, beats: number): [GridAnchor, number] {
  let after = 0;
  while (after < tempo.anchors.length && tempo.anchors[after].beat <= beats) after++;
  return stretch(tempo, after);
}

/** The beats from bar 1's first to `timeS` (fractional, below zero before it). */
export function beatsAt(tempo: Tempo, timeS: number): number {
  const [from, perS] = stretchAtTime(tempo, timeS);
  return from.beat + (timeS - from.timeS) * perS;
}

/** Where beat `beats` (fractional) falls. */
export function timeOf(tempo: Tempo, beats: number): number {
  const [from, perS] = stretchAtBeat(tempo, beats);
  return from.timeS + (beats - from.beat) / perS;
}

/** The tempo at `timeS`. */
export function bpmAt(tempo: Tempo, timeS: number): number {
  return stretchAtTime(tempo, timeS)[1] * 60;
}
