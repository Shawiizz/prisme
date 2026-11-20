import { VinylSyncMode } from './engine';
import { Shortcut } from '../keyboard/shortcuts';

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

/** How the interface shows itself. */
export interface UiPreferences {
  /** `null`: the system's. */
  language: string | null;
  /** On a phone, the decks' touch controls shown or not; `null`: as what is plugged in says. */
  phoneTouch: boolean | null;
  /** Spread over the screen under the main one. */
  screenBelow: boolean;
}

/** What the settings show of the preferences (`commands/preferences.rs`), and what they offer. */
export interface Preferences {
  dj: DjPreferences;
  library: LibraryPreferences;
  ui: UiPreferences;
  /** The keyboard's shortcuts in use. */
  shortcuts: Shortcut[];
  /** The actions whose keys the DJ changed. */
  changed: string[];
  choices: {
    tempoRanges: number[];
    deckCounts: number[];
    talkoverDepths: number[];
    languages: string[];
  };
}

/** Each setting the settings change, and its value (`commands::preferences::Setting`). */
export interface Settings {
  vinylSync: VinylSyncMode;
  autoGain: boolean;
  deckCount: number;
  knobMagnet: boolean;
  lockControls: boolean;
  cueOnLoad: boolean;
  protectPlaying: boolean;
  hiddenColumns: string[];
  writeTags: boolean;
  replaceTags: boolean;
  pauseWhilePlaying: boolean;
  stemsOnLoad: boolean;
  stemsGpu: boolean;
  stemsFast: boolean;
  language: string | null;
  phoneTouch: boolean | null;
}
