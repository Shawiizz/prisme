/** Shapes exchanged with the Rust side (see apps/prisme-desktop/src/dto.rs). */

export type SignalState = 'none' | 'bounce' | 'ok';
export type VinylMode = 'absolute' | 'relative';

export interface VinylStatus {
  enabled: boolean;
  mode: VinylMode;
  signal: SignalState;
  recordSpeed: number;
  recordPositionS: number | null;
}

export interface DeckStatus {
  loaded: boolean;
  playing: boolean;
  positionS: number;
  durationS: number;
  speed: number;
  vinyl: VinylStatus;
}

export interface EngineStatus {
  audioRunning: boolean;
  decks: DeckStatus[];
  mainPeak: number;
}

export interface Waveform {
  slicesPerSecond: number;
  peak: number[];
  low: number[];
  mid: number[];
  high: number[];
}

export interface TrackInfo {
  deck: number;
  path: string;
  title: string;
  artist: string | null;
  durationS: number;
  sampleRate: number;
  waveform: Waveform;
}

/** Deck settings owned by the UI and pushed to the engine. */
export interface DeckSettings {
  pitch: number;
  volume: number;
  vinylEnabled: boolean;
  vinylMode: VinylMode;
  leadInS: number;
  cueS: number;
}

/** Settings as the backend applies them (`cueS` is UI-only). */
export interface MixerState {
  decks: Omit<DeckSettings, 'cueS'>[];
  crossfader: number;
}

export const DECK_COUNT = 2;
