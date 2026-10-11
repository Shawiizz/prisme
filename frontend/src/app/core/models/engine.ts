/** Shapes exchanged with the Rust side (see apps/prisme-desktop/src/dto.rs). */

export type SignalState = 'none' | 'bounce' | 'ok';
export type VinylMode = 'absolute' | 'relative';

export interface VinylStatus {
  enabled: boolean;
  mode: VinylMode;
  /** The needle reached the end of the record: the deck plays on by itself. */
  runOut: boolean;
  /** The record has read backwards at play speed for a while: crossed channels? */
  crossed: boolean;
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
  /** Start marked by LOOP IN, waiting for LOOP OUT. */
  loopInS: number | null;
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
  /** Sampler slots playing, one bit per slot. */
  samplesPlaying: number;
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
export const MAX_KEY_SHIFT = 12;
export const SAMPLE_SLOTS = 8;

/** What SYNC does on a record in relative mode. */
export type VinylSyncMode = 'once' | 'locked';

/** How well the audio keeps up. */
export interface AudioHealth {
  /** Periods processed too late: each one is an audible click. */
  overBudget: number;
  worstCallbackUs: number;
  inputUnderruns: number;
  deviceLost: boolean;
  /** The engine failed and could not be restarted: the audio is silent. */
  engineFailed: boolean;
}

export interface EngineStatus {
  audioRunning: boolean;
  decks: DeckStatus[];
  mainPeak: number;
  health: AudioHealth | null;
  recording: RecordingStatus | null;
}

export interface RecordingStatus {
  seconds: number;
  /** Audio the writer could not take in time (lost). */
  droppedSeconds: number;
}

/** What a recording takes: the software mixer's master, or an input pair (0-based). */
export type RecordSource = { kind: 'master' } | { kind: 'input'; first: number };

export interface Recording {
  path: string;
  seconds: number;
  /** Tracks in its tracklist. */
  tracks: number;
}

/** One byte per slice in each band (sent in binary, see `decodeWaveform`). */
export interface Waveform {
  slicesPerSecond: number;
  peak: Uint8Array;
  low: Uint8Array;
  mid: Uint8Array;
  high: Uint8Array;
}

/** `slices_per_second` (u32 little-endian), then the peak, low, mid and high bands. */
export function decodeWaveform(buffer: ArrayBuffer): Waveform | null {
  if (buffer.byteLength < 4) return null;
  const slices = (buffer.byteLength - 4) / 4;
  const band = (i: number) => new Uint8Array(buffer, 4 + i * slices, slices);
  return {
    slicesPerSecond: new DataView(buffer).getUint32(0, true),
    peak: band(0),
    low: band(1),
    mid: band(2),
    high: band(3),
  };
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
  /** Tells this load from another of the same file. */
  loadId: number;
  path: string;
  title: string;
  artist: string | null;
  durationS: number;
  sampleRate: number;
  /** The waveform can be fetched; otherwise a `deck-track-update` says when it can. */
  waveformReady: boolean;
  tempo: Tempo | null;
  key: MusicalKey | null;
  /** Corrected by hand. */
  gridEdited: boolean;
  keyEdited: boolean;
  cues: Cue[];
  /** Semitones the deck transposes it by (0 at each load). */
  keyShift: number;
}

/** What became known of a loaded track after it was put on its deck. */
export interface DeckTrackUpdate {
  deck: number;
  loadId: number;
  waveformReady: boolean;
  analysis: TrackAnalysis | null;
}

/** Tempo and key of a loaded track, after a correction or its analysis. */
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
  /** The pitch stays when the speed changes. */
  keyLock: boolean;
  strip: ChannelStrip;
  fx: FxSettings;
  /** Name of the sound in each sampler slot. */
  samples: (string | null)[];
  samplerGain: number;
}

export type EffectKind =
  'echo' | 'pingPong' | 'reverb' | 'flanger' | 'phaser' | 'filter' | 'gate' | 'bitcrusher';

/** A deck's beat effect. */
export interface FxSettings {
  kind: EffectKind;
  enabled: boolean;
  /** Level of the effect, 0..1. */
  mix: number;
  /** The effect's main knob, 0..1. */
  amount: number;
  /** The effect's period in beats. */
  beats: number;
}

export const DEFAULT_FX: FxSettings = {
  kind: 'echo',
  enabled: false,
  mix: 0.5,
  amount: 0.5,
  beats: 1,
};
/** Shortest and longest effect period, in beats. */
export const FX_BEATS = { min: 1 / 8, max: 8 };

/** EQ bands, in the order of `ChannelStrip.eq`. */
export type Band = 'low' | 'mid' | 'high';
export const BANDS: readonly Band[] = ['low', 'mid', 'high'];

/** A channel of the software mixer, before its fader. Gains are linear. */
export interface ChannelStrip {
  trim: number;
  /** Low, mid, high. */
  eq: number[];
  kills: boolean[];
  /** -1 = low-pass closed, 0 = off, 1 = high-pass open. */
  filter: number;
}

export const FLAT_STRIP: ChannelStrip = {
  trim: 1,
  eq: [1, 1, 1],
  kills: [false, false, false],
  filter: 0,
};

/** Smooth: equal power, for blending. Sharp: cuts only at the very ends, for scratching. */
export type CrossfaderCurve = 'smooth' | 'sharp';

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
  crossfaderCurve: CrossfaderCurve;
  /** Master limiter. */
  limiter: boolean;
}

export const DECK_COUNT = 2;
