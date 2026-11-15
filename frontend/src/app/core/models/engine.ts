/** Shapes exchanged with the Rust side (see apps/prisme-desktop/src/dto.rs). */

import { StemPart } from '../stem-parts';
import { Separation, StemMix } from './stems';

export type SignalState = 'none' | 'bounce' | 'ok';
export type VinylMode = 'absolute' | 'relative';
/** The speed the record turns at when the track plays at its own speed. */
export type Rpm = '33' | '45';

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
  /** The stems whose change waits for the next bar, a bit each: vocals, drums, bass, other. */
  stemsWaiting: number;
  /** The parts separated where the deck plays, the same bits. */
  stemsHere: number;
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
  /** Input frames dropped: more had come than the output took. */
  inputDropped: number;
  deviceLost: boolean;
  /** The engine failed and could not be restarted: the audio is silent. */
  engineFailed: boolean;
  /** Frames per period, as the driver calls the engine (0 before the first period). */
  periodFrames: number;
}

export interface EngineStatus {
  audioRunning: boolean;
  decks: DeckStatus[];
  mainPeak: number;
  /** The microphone and the aux line at their levels, on or not. */
  micPeak: number;
  auxPeak: number;
  health: AudioHealth | null;
  recording: RecordingStatus | null;
  /** The track heard in the headphones before it goes on a deck. */
  preview: PreviewStatus | null;
}

export interface PreviewStatus {
  /** The preview's (`PreviewTrack.id`). */
  id: number;
  positionS: number;
  /** Estimated until the track is all read. */
  durationS: number;
  /** Its audio is not read that far yet: it waits, silent. */
  waiting: boolean;
}

/** A track previewed in the headphones (`preview_track`). */
export interface PreviewTrack {
  id: number;
  path: string;
  title: string;
  artist: string | null;
  /** Estimated until the track is all read. */
  durationS: number;
  /** The waveform can be fetched; otherwise a `preview-update` with the id says when. */
  waveformReady: boolean;
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
  /** The separated stems on the deck, which it can mute. */
  separation: Separation;
  /** The parts the deck plays (all at each load). */
  stemMix: StemMix;
}

/** What became known of a loaded track after it was put on its deck. */
export interface DeckTrackUpdate {
  deck: number;
  loadId: number;
  waveformReady: boolean;
  analysis: TrackAnalysis | null;
  /** Its separated stems reached the deck. */
  separation: Separation | null;
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
  rpm: Rpm;
  /** Where the track starts on the record, if chosen: else the record's own. */
  leadInS: number | null;
  /** The turntable's left and right channels are crossed. */
  swapChannels: boolean;
  /** The pitch stays when the speed changes. */
  keyLock: boolean;
  /** Software mixer: the channel fader raised from closed plays the deck, closed it stops it
   * on its cue point. */
  faderStart: boolean;
  strip: ChannelStrip;
  fx: FxChain;
  /** Name of the sound in each sampler slot. */
  samples: (string | null)[];
  samplerGain: number;
  /** The deck plays its input as it comes instead of its track. */
  passthrough: boolean;
  /** At the end, the track goes on from its start. */
  repeat: boolean;
}

export const EFFECT_KINDS = [
  'echo',
  'pingPong',
  'reverb',
  'flanger',
  'phaser',
  'filter',
  'gate',
  'bitcrusher',
] as const;
export type EffectKind = (typeof EFFECT_KINDS)[number];

/** A slot of a deck's effect chain. */
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

/** A deck's beat effects, played one slot after the other. */
export interface FxChain {
  /** `FX_SLOTS` of them. */
  slots: FxSettings[];
  /** The part of the track the chain is on, where separated; `null`: the whole deck. */
  part: StemPart | null;
}

export const FX_SLOTS = 3;
const DEFAULT_FX_SLOT: FxSettings = {
  kind: 'echo',
  enabled: false,
  mix: 0.5,
  amount: 0.5,
  beats: 1,
};
export const DEFAULT_FX: FxChain = {
  slots: Array(FX_SLOTS).fill(DEFAULT_FX_SLOT),
  part: null,
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

/** Where the software mixer sends the headphones; `first` is 0-based. */
export type HeadphonesOutput = { kind: 'auto' } | { kind: 'off' } | { kind: 'pair'; first: number };

/** The input channels a live source comes in on (`first` 0-based): one, heard in the middle,
 * or a stereo pair. */
export interface InputChannels {
  first: number;
  stereo: boolean;
}

/** The live sources of the software mixer. */
export type LiveSource = 'mic' | 'aux';

export interface LiveState {
  /** The inputs chosen. */
  input: InputChannels | null;
  /** They are on the running device, left free by the turntables. */
  inUse: boolean;
  /** Linear gain. */
  level: number;
  /** Heard in the master. */
  on: boolean;
  /** Heard in the headphones. */
  cue: boolean;
}

/** Settings as the backend applies them. */
export interface MixerState {
  decks: DeckSettings[];
  crossfader: number;
  mode: MixMode;
  /** An external mixer needs an output pair per deck; `null` without audio. */
  modeInUse: MixMode | null;
  headphones: HeadphonesOutput;
  /** First output (0-based) of the headphones in use; `null` when they are not heard. With an
   * external mixer, a pair no deck uses, for the preview alone. */
  headphonesInUse: number | null;
  /** Outputs of the running device; `null` without audio. */
  outputChannels: number | null;
  /** Headphones: 0 = cued decks only, 1 = master only. */
  cueMix: number;
  headphoneGain: number;
  crossfaderCurve: CrossfaderCurve;
  /** Master limiter. */
  limiter: boolean;
  /** -1 = the master on the left only, 1 = on the right only. */
  balance: number;
  /** First output (0-based) of the booth chosen, and of the booth in use. */
  booth: number | null;
  boothInUse: number | null;
  boothGain: number;
  mic: LiveState;
  aux: LiveState;
  /** While the microphone is on, the music ducks by `talkoverDepthDb`. */
  talkover: boolean;
  talkoverDepthDb: number;
  /** The inputs of the running device the turntables leave free (0-based). */
  freeInputs: number[];
}

/** The decks there can be: two are shown, or all four (a preference). */
export const MAX_DECKS = 4;
