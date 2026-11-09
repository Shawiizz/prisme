/** How far a track is separated: its vocals (the rest is the instrumental), or its vocals,
 * drums and bass (the rest is the other instruments). */
export type Separation = 'none' | 'vocals' | 'full';

/** Which parts of the track a deck plays. */
export interface StemMix {
  vocals: boolean;
  drums: boolean;
  bass: boolean;
  /** What no stem holds: the instrumental of a track separated into its vocals alone. */
  other: boolean;
}

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
