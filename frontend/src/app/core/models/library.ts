import { MusicalKey } from './engine';

export interface TrackTags {
  title: string | null;
  artist: string | null;
  album: string | null;
  genre: string | null;
  year: number | null;
}

/** What the library knows of a track: analysis and tags (see TrackDetailsDto). */
export interface TrackDetails {
  bpm: number | null;
  key: MusicalKey | null;
  tags: TrackTags;
  /** The file could not be decoded. */
  failed: boolean;
}

export interface DirEntry {
  name: string;
  path: string;
  /** `null` until the track has been analysed. */
  details: TrackDetails | null;
}

export interface Crate {
  id: number;
  name: string;
  trackCount: number;
}

/** A run of the app that played tracks. */
export interface HistorySession {
  id: number;
  startedAtMs: number;
  playCount: number;
}

export interface HistoryPlay extends DirEntry {
  deck: number;
  playedAtMs: number;
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
  details: TrackDetails;
}
