/** Shapes exchanged with the Rust side (see apps/prisme-desktop/src/dto.rs). */

export type SignalState = 'none' | 'bounce' | 'ok';
export type VinylMode = 'absolute' | 'relative';

export interface VinylStatus {
  enabled: boolean;
  mode: VinylMode;
  signal: SignalState;
  /** Timecode carrier level (full scale = 1). */
  level: number;
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
  /** Loudest sample since the previous status. */
  peak: number;
  cue: boolean;
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

/** How well the audio keeps up. */
export interface AudioHealth {
  /** Periods processed too late: each one is an audible click. */
  overBudget: number;
  worstCallbackUs: number;
  inputUnderruns: number;
  deviceLost: boolean;
}

export interface EngineStatus {
  audioRunning: boolean;
  decks: DeckStatus[];
  mainPeak: number;
  health: AudioHealth | null;
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
  /** Corrected by hand. */
  gridEdited: boolean;
  keyEdited: boolean;
  cues: Cue[];
}

/** Tempo and key of a loaded track, after a correction. */
export type TrackAnalysis = Pick<TrackInfo, 'tempo' | 'key' | 'gridEdited' | 'keyEdited'>;

/** A correction of the beat grid (see `edit_grid`). */
export type GridChange =
  | { kind: 'bpm'; bpm: number }
  | { kind: 'scale'; factor: number }
  | { kind: 'shift'; seconds: number }
  | { kind: 'beatHere' }
  | { kind: 'reset' };

/** Deck settings owned by the UI and pushed to the engine. */
export interface DeckSettings {
  pitch: number;
  volume: number;
  /** Software mixer: heard in the headphones. */
  cue: boolean;
  vinylEnabled: boolean;
  vinylMode: VinylMode;
  leadInS: number;
  /** The turntable's left and right channels are crossed. */
  swapChannels: boolean;
}

/** External: each deck on its own outputs for a hardware mixer. Internal: the software mixer. */
export type MixMode = 'external' | 'internal';

/** Settings as the backend applies them. */
export interface MixerState {
  decks: DeckSettings[];
  crossfader: number;
  mode: MixMode;
  /** An external mixer needs an output pair per deck; `null` without audio. */
  modeInUse: MixMode | null;
  /** Headphones: 0 = cued decks only, 1 = master only. */
  cueMix: number;
  headphoneGain: number;
}

export const DECK_COUNT = 2;
