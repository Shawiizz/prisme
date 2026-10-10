import { MusicalKey } from './engine';

/** Stored analysis of a track (see TrackAnalysisDto). */
export interface TrackAnalysis {
  bpm: number | null;
  key: MusicalKey | null;
  /** The file could not be decoded. */
  failed: boolean;
}

export interface DirEntry {
  name: string;
  path: string;
  sizeBytes: number;
  /** `null` until the track has been analysed. */
  analysis: TrackAnalysis | null;
}

export interface DirListing {
  path: string;
  parent: string | null;
  folders: DirEntry[];
  tracks: DirEntry[];
}

/** Background analysis of the music folder (`library-progress` event). */
export interface AnalysisProgress {
  total: number;
  done: number;
  failed: number;
  running: boolean;
}

/** Payload of the `track-analyzed` event. */
export interface TrackAnalyzed {
  path: string;
  analysis: TrackAnalysis;
}
