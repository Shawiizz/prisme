/** Which stems a deck plays. */
export interface StemMix {
  vocals: boolean;
  instrumental: boolean;
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

/** The separation model: installed, or how far its download is (0 to 1). */
export interface ModelStatus {
  installed: boolean;
  downloading: number | null;
  error: string | null;
  bytes: number;
}

export interface StemsStatus {
  model: ModelStatus;
  progress: StemsProgress;
}
