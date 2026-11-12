import { VinylSyncMode } from './engine';

/** Shapes of apps/prisme-desktop/src/preferences.rs. */

export interface DjPreferences {
  vinylSync: VinylSyncMode;
  quantize: boolean;
  /** Range of the tempo faders (0.08 = ±8 %). */
  pitchRange: number;
  /** Every track played at the same loudness. */
  autoGain: boolean;
  /** Two decks, or four. */
  deckCount: number;
}

export interface LibraryPreferences {
  lastFolder: string | null;
  /** Columns of the track list the DJ hid. */
  hiddenColumns: string[];
  /** Tempo and key written into the files' tags. */
  writeTags: boolean;
  /** Those another program wrote (Mixed In Key, say) replaced as well. */
  replaceTags: boolean;
  /** The background analysis of the music folder waits while a deck plays. */
  pauseWhilePlaying: boolean;
  /** A track loaded on a deck is separated right away, before the library's; one put in the
   * prepare list in the background. */
  /** `null`: not chosen, on with a graphics card that can separate. */
  separateOnLoad: boolean | null;
}

export interface Preferences {
  dj: DjPreferences;
  library: LibraryPreferences;
}

export const PITCH_RANGES = [0.08, 0.16, 0.5] as const;
