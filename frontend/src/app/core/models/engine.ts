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

export interface SyncStatus {
  locked: boolean;
  leader: boolean;
  /** Relative vinyl: factor a one-shot sync applies to the platter speed. */
  rateOffset: number;
}

export interface LoopStatus {
  startS: number | null;
  endS: number | null;
  active: boolean;
}

export interface DeckStatus {
  loaded: boolean;
  playing: boolean;
  positionS: number;
  durationS: number;
  speed: number;
  /** Current tempo, gestures excluded (`null` without a beat grid). */
  bpm: number | null;
  /** Position within the current beat, 0..1. */
  beatPhase: number | null;
  sync: SyncStatus;
  looping: LoopStatus;
  slipMode: boolean;
  /** Where the track would be without the loop, roll or censor in progress. */
  slipPositionS: number | null;
  vinyl: VinylStatus;
}

export interface Cue {
  /** Hot cue slot, `null` for the main cue. */
  hot: number | null;
  positionS: number;
  /** A saved loop starting at `positionS`. */
  loopLengthS: number | null;
  /** 0xRRGGBB */
  color: number;
  label: string;
}

export const HOT_CUES = 8;

/** What SYNC does on a record in relative mode. */
export type VinylSyncMode = 'once' | 'locked';

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

export interface Tempo {
  bpm: number;
  /** First beat at or after the start of the track (seconds). */
  firstBeatS: number;
}

export interface MusicalKey {
  /** "Am", "F#"... */
  name: string;
  /** "8A", "2B"... */
  camelot: string;
}

export interface TrackInfo {
  deck: number;
  path: string;
  title: string;
  artist: string | null;
  durationS: number;
  sampleRate: number;
  waveform: Waveform;
  tempo: Tempo | null;
  key: MusicalKey | null;
  cues: Cue[];
}

/** Deck settings owned by the UI and pushed to the engine. */
export interface DeckSettings {
  pitch: number;
  volume: number;
  vinylEnabled: boolean;
  vinylMode: VinylMode;
  leadInS: number;
}

/** Settings as the backend applies them. */
export interface MixerState {
  decks: DeckSettings[];
  crossfader: number;
}

export const DECK_COUNT = 2;
