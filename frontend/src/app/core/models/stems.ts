/** How far a track is separated: its vocals (the rest is the instrumental), or its vocals,
 * drums and bass (the rest is the other instruments). */
export type Separation = 'none' | 'vocals' | 'full';

/** How a deck plays each part of the track. */
export interface StemMix {
  vocals: StemLevel;
  drums: StemLevel;
  bass: StemLevel;
  /** What no stem holds: the instrumental of a track separated into its vocals alone. */
  other: StemLevel;
}

/** A part's level (0 to 1), kept while it is muted. */
export interface StemLevel {
  level: number;
  on: boolean;
}

/** Every part at its level in the track. */
export const FULL_MIX: StemMix = {
  vocals: { level: 1, on: true },
  drums: { level: 1, on: true },
  bass: { level: 1, on: true },
  other: { level: 1, on: true },
};

/** The separation queue. */
export interface StemsProgress {
  running: boolean;
  /** Tracks left, the one being separated included. */
  queued: number;
  done: number;
  failed: number;
  /** The track being separated, and how far (0 to 1). */
  current: string | null;
  fraction: number;
  /** The GPU (and API) or "CPU" doing the work. */
  device: string | null;
}

/** The two separation models: the vocals one, then the drums and bass one. */
export type ModelId = 'vocals' | 'drums-bass';

/** A separation model: installed, or how far its download is (0 to 1). */
export interface ModelStatus {
  id: ModelId;
  installed: boolean;
  downloading: number | null;
  error: string | null;
  bytes: number;
}

export interface StemsStatus {
  models: ModelStatus[];
  progress: StemsProgress;
}

/** A track whose stems were just stored. */
export interface Separated {
  path: string;
  separation: Separation;
}
