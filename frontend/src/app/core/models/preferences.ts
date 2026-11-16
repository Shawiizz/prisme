import { VinylSyncMode } from './engine';
import { KeyChanges } from '../keyboard/shortcuts';

/** Shapes of apps/prisme-desktop/src/preferences.rs. */

export interface DjPreferences {
  vinylSync: VinylSyncMode;
  quantize: boolean;
  /** Range of the tempo faders of the decks without one of their own (0.08 = ±8 %). */
  pitchRange: number;
  /** Every track played at the same loudness. */
  autoGain: boolean;
  /** Two decks, or four. */
  deckCount: number;
  /** The knobs of the mixer hold at their middle when turned through it. */
  knobMagnet: boolean;
  /** On a phone, the decks play and pause from its lock screen and its notifications. */
  lockControls: boolean;
  /** The headphones go to the deck a track is loaded on, alone. */
  cueOnLoad: boolean;
  /** A load onto a deck that plays is refused, rather than asked about. */
  protectPlaying: boolean;
}

export interface LibraryPreferences {
  /** The music folder chosen (`null`: the system's); changed with `set_music_folder`. */
  musicFolder: string | null;
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
  stemsOnLoad: boolean | null;
  /** The separation runs on the graphics chip when there is one worth it. */
  stemsGpu: boolean;
  /** The small separation models, faster, rather than the large ones. */
  stemsFast: boolean;
}

export interface Preferences {
  dj: DjPreferences;
  library: LibraryPreferences;
  /** The keys the DJ changed, by control. */
  keys: KeyChanges;
}

export const PITCH_RANGES = [0.08, 0.16, 0.5] as const;

/** The range `step` places after `range` among `PITCH_RANGES` (before it, negative), round
 * from the last to the first; the first for a range not among them. */
export function nextTempoRange(range: number, step: number): number {
  const at = PITCH_RANGES.indexOf(range as (typeof PITCH_RANGES)[number]);
  if (at < 0) return PITCH_RANGES[0];
  const n = PITCH_RANGES.length;
  return PITCH_RANGES[(((at + step) % n) + n) % n];
}
