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

/** The separation models: the vocals one, the drums and bass one, and a small vocals one,
 * faster. */
export type ModelId = 'vocals' | 'drums-bass' | 'four-fast';

/** A separation model as the settings list it: installed, or how far its download is (0 to
 * 1); `ready` to be downloaded (the drums and bass come with the vocals only). */
export interface ModelRow {
  id: ModelId;
  installed: boolean;
  downloading: number | null;
  error: string | null;
  bytes: number;
  ready: boolean;
}

/** A choice of models: what it has left to download (`null` once installed), and why its
 * download failed. */
export interface ModelChoice {
  bytesLeft: number | null;
  error: string | null;
}

/** The separation as the backend tells it. */
export interface StemsState {
  progress: StemsProgress;
  /** The tracks the queue held: done, failed and left. */
  total: number;
  /** The queue's share done (0 to 1). */
  overall: number;
  /** How far the models chosen and installed separate a track. */
  reach: Separation;
  /** A graphics chip worth separating on is there; `null` until looked for. */
  gpu: boolean | null;
  /** The separation runs on it. */
  onGpu: boolean;
  /** Tracks are separated as they are loaded or prepared. */
  onLoad: boolean;
  /** How far the downloads under way are (0 to 1). */
  downloading: number | null;
  /** The models the DJ chose. */
  models: ModelRow[];
  quality: ModelChoice;
  fast: ModelChoice;
  /** The small models suit this device. */
  fastSuits: boolean;
  /** The choice the models are offered with. */
  suggestFast: boolean;
}

/** How far a row's track is separated, as its column shows it. */
export type RowStems =
  | { state: 'separating'; fraction: number }
  | { state: 'queued' }
  | { state: 'addable'; separation: Separation }
  | { state: 'separated'; separation: Separation };
