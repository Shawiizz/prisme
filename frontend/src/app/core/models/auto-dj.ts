export type AutoDjState = 'off' | 'on' | 'mixing' | 'paused';
export type AutoDjReason =
  | 'externalMixer'
  | 'records'
  | 'listEmpty'
  | 'listEnded'
  | 'transport'
  | 'crossfader'
  | 'load'
  | 'unloadable';

/** The Auto DJ as the backend runs it (see apps/prisme-desktop/src/auto_dj). */
export interface AutoDjStatus {
  state: AutoDjState;
  /** Why it stopped, paused, or left a track out. */
  reason: AutoDjReason | null;
  /** The deck the DJ played or loaded, for those reasons. */
  reasonDeck: number | null;
  /** The deck heard. */
  current: number | null;
  /** The deck being mixed in. */
  incoming: number | null;
  /** Where on the deck heard the next mix starts. */
  mixAtS: number | null;
  /** Length of a mix between tracks with a beat grid. */
  fadeBeats: number;
  /** Length of a mix when one has none. */
  fadeS: number;
}
