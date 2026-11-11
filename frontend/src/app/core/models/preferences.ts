import { VinylSyncMode } from './engine';

/** Shapes of apps/prisme-desktop/src/preferences.rs. */

export interface DjPreferences {
  vinylSync: VinylSyncMode;
  quantize: boolean;
  /** Range of the tempo faders (0.08 = ±8 %). */
  pitchRange: number;
}

export interface LibraryPreferences {
  lastFolder: string | null;
  /** Columns of the track list the DJ hid. */
  hiddenColumns: string[];
  /** Tempo and key written into the files' tags. */
  writeTags: boolean;
  /** A track loaded on a deck is separated right away, before the library's. */
  separateOnLoad: boolean;
}

export interface Preferences {
  dj: DjPreferences;
  library: LibraryPreferences;
}

export const PITCH_RANGES = [0.08, 0.16, 0.5] as const;
