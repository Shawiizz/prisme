import { Preferences } from './preferences';
/** Shapes exchanged with the Rust side (see apps/prisme-desktop/src/dto.rs). */

import { Knob } from '../../shared/readout';
import { MeterLevel } from '../../shared/meter';
import { StemPart } from '../stem-parts';
import { LibraryLists, LibraryView } from './library';
import { Separation, StemMix, StemsState } from './stems';

export type SignalState = 'none' | 'bounce' | 'ok';
export type VinylMode = 'absolute' | 'relative';
/** The speed the record turns at when the track plays at its own speed. */
export type Rpm = '33' | '45';
/** A setting of a deck's vinyl control, changed alone: how the record drives the deck, how
 * fast it turns, where on it the track starts (`null`: where its own lead-in ends). */
export type VinylChange =
  | { setting: 'mode'; value: VinylMode }
  | { setting: 'rpm'; value: Rpm }
  | { setting: 'leadIn'; value: number | null };

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

/** Who drives a deck: itself (INT), its record in relative or absolute mode, or no one: it
 * plays its input as it comes (THRU, a real record or a CD player). */
export type DeckMode = 'int' | VinylMode | 'thru';

/** A deck's sync: off, matched once on a record, or locked to the leader. */
export type SyncState = 'off' | 'matched' | 'locked';

/** What a deck's SYNC key does: lock (held: match once), match once, or nothing for want of a
 * beat grid or because its record holds the place. */
export type SyncKey = 'lock' | 'once' | 'noGrid' | 'absolute';

export interface SyncStatus {
  state: SyncState;
  leader: boolean;
  key: SyncKey;
}

/** A light's state, as the controllers' lights have it. */
export type LightState = 'on' | 'off' | 'paused' | 'loaded' | 'playing' | 'soon';

/** A deck's tempo fader as shown. */
export interface TempoFader {
  /** It does not set the tempo: a record does, or sync. */
  held: boolean;
  /** The tempo it shows, as a rate: the one played while held, else its own. */
  ratio: number;
  /** Where it stands, 0 to 1. */
  position: number;
  /** On a record not locked by sync: the rate matching the deck it follows. */
  matching: number | null;
  /** Where that rate is along the fader, within its range. */
  matchingAt: number | null;
}

export interface LoopStatus {
  startS: number | null;
  endS: number | null;
  /** The beats of the loop playing, as the pads of a loop of that length show it. */
  beats: number | null;
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
  /** The rate it plays at: its tempo, gestures excluded, or its fader's without a track. */
  rate: number;
  /** Current tempo, gestures excluded (`null` without a beat grid). */
  bpm: number | null;
  /** Position within the current beat, 0..1. */
  beatPhase: number | null;
  /** The key its track is heard in, moved by the rate unless held. */
  heardKey: MusicalKey | null;
  /** The deck it is mixed with. */
  mixedWith: number;
  /** The DJ hears or holds its track: it plays, or moves under its record. */
  plays: boolean;
  /** The time left of its track (s). */
  remainingS: number | null;
  /** The end of its track near (on), or soon. */
  ending: LightState;
  mode: DeckMode;
  tempo: TempoFader;
  sync: SyncStatus;
  /** Its loops, jumps and pads may move its track. */
  canMove: boolean;
  /** Its motor (brake, spin back) may move its track: it plays it by itself. */
  canMotor: boolean;
  /** Its grid may move onto the beats of the deck it is mixed with. */
  canAlignGrid: boolean;
  /** Its key may be moved to mix with the one heard on the deck it is mixed with. */
  canSyncKey: boolean;
  looping: LoopStatus;
  cue: boolean;
  slipMode: boolean;
  /** The reverse is on: the track plays backwards. */
  reverse: boolean;
  /** Where the track would be without the loop, roll, censor or reverse in progress. */
  slipPositionS: number | null;
  vinyl: VinylStatus;
  /** What its keys and pads light, by the controllers' rules. */
  lights: DeckLights;
  /** Sampler slots playing, one bit per slot (16). */
  samplesPlaying: number;
  /** The parts of its separated track, a knob each. */
  stems: StemKnob[];
  /** How far its track's separation is (0 to 1): as it is separated, or as the downloads of the
   * models it waits for are. */
  separating: number | null;
}

export interface DeckLights {
  /** CUE: on at the cue point, paused stopped elsewhere. */
  cue: LightState;
  /** The slice of its slicer it plays in. */
  slice: number | null;
}

/** A part of a deck's separated track, as its knob shows it. */
export interface StemKnob {
  part: StemPart;
  level: number;
  on: boolean;
  /** Its change waits for the next bar. */
  waiting: boolean;
  /** Separated where the deck plays. */
  ready: boolean;
}

/** What the strip between the decks tells of the second deck's beats against the first's: off
 * the beat within so many seconds at their tempos (0: already), matched, or ahead or behind by
 * so many milliseconds. */
export type PhaseReading =
  | { state: 'off' | 'matched' }
  | { state: 'drifting'; seconds: number }
  | { state: 'ahead' | 'behind'; ms: number };

/** The strip comparing two decks' beats: the first two playing, else A and B. */
export type PhaseStrip = { decks: [number, number] } & PhaseReading;

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

/** Two pages of eight pads. */
export const HOT_CUES = 16;
export const SAMPLE_SLOTS = 16;
/** The pads of a deck: a page of hot cues or sounds. */
export const PADS = 8;
/** The slicer's slices: two bars of the beat grid, a beat each. */
export const SLICES = 8;

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
  /** What the meters show: every deck's, the master's, the live sources' (on or not). */
  meters: { decks: MeterLevel[]; main: MeterLevel; mic: MeterLevel; aux: MeterLevel };
  health: AudioHealth | null;
  recording: RecordingStatus | null;
  /** The track heard in the headphones before it goes on a deck. */
  preview: PreviewStatus | null;
  phase: PhaseStrip;
  /** The sections of what the app mirrors the backend changed since the status before: the
   * mixer (with the quantize and the tempo range), the tracks (with their loads), the pads,
   * the preview. All of them in the first status. */
  controls?: Partial<ControlState>;
}

/** What the app mirrors of the decks, their tracks, the mixer and the DJ's preferences. */
export interface ControlState {
  mixer: MixerState;
  /** Each deck's track by its load, and what changes of it while it stays there. */
  tracks: (TrackState | null)[];
  /** Whether each deck loads a track. */
  loading: boolean[];
  quantize: boolean;
  /** The range of the tempo faders without one of their own. */
  pitchRange: number;
  /** What each deck's pads hold. */
  perform: DeckPerform[];
  preview: PreviewState;
  /** The list browsed, and the rows of the window the table asked for. */
  library: LibraryView;
  /** The lists the library keeps: the crates, the sets, the prepare list. */
  lists: LibraryLists;
  preferences: Preferences;
  stems: StemsState;
}

/** What the headphones preview: the track the engine was given, and the file opening for the
 * preview asked last. */
export interface PreviewState {
  track: PreviewTrack | null;
  opening: string | null;
}

export interface DeckPerform {
  /** The fader cut playing (an index of `CUT_BEATS`). */
  cut: number | null;
  /** The roll playing, in beats. */
  roll: number | null;
  /** The slot of its effect chain the effect knobs turn. */
  fxFocus: number | null;
}

export interface TrackState {
  /** The load it is of: another load of the deck is not changed. */
  loadId: number;
  keyShift: number;
  stemMix: StemMix;
  cues: Cue[];
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

/** A track previewed in the headphones. */
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

/** A recording ended: where it went, and what could not be written. */
export interface Recording {
  /** Its first file. */
  path: string;
  /** Every file it was written in: the first, then `… 2.wav` past 4 GB or an audio restart. */
  files: string[];
  seconds: number;
  /** Tracks in its tracklist. */
  tracks: number;
  /** The tracklist written beside it, if it could be. */
  tracklist: string | null;
  /** Why the audio was not written whole (a disk full, a drive gone), if it was not. */
  failure: string | null;
}

/** A waveform as it is drawn (sent in binary, see `decodeWaveform`): for each slice known,
 * from the start of the track, the height of its envelope and its colour, a byte each. */
export interface Waveform {
  slicesPerSecond: number;
  /** The track's slices: those known, and those still read. */
  slices: number;
  height: Uint8Array;
  red: Uint8Array;
  green: Uint8Array;
  blue: Uint8Array;
}

/** The track's slices and `slices_per_second` (u32 little-endian each), then the height, red,
 * green and blue of each slice known. */
export function decodeWaveform(buffer: ArrayBuffer): Waveform | null {
  if (buffer.byteLength <= 8) return null;
  const header = new DataView(buffer);
  const known = (buffer.byteLength - 8) / 4;
  const band = (i: number) => new Uint8Array(buffer, 8 + i * known, known);
  return {
    slices: header.getUint32(0, true),
    slicesPerSecond: header.getUint32(4, true),
    height: band(0),
    red: band(1),
    green: band(2),
    blue: band(3),
  };
}

export interface Tempo {
  bpm: number;
  /** First beat at or after the start of the track (seconds). */
  firstBeatS: number;
  /** The beats of a bar, the first on `firstBeatS`. */
  beatsPerBar: number;
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
  /** Its stems separated in parts only: being separated, or a separation stopped short. */
  partial: boolean;
  /** The parts the deck plays (all at each load). */
  stemMix: StemMix;
}

/** What became known of a loaded track after it was put on its deck. */
export interface DeckTrackUpdate {
  deck: number;
  loadId: number;
  analysis: TrackAnalysis | null;
  /** Its separated stems reached the deck, or its separation stopped short. */
  stems: Pick<TrackInfo, 'separation' | 'partial'> | null;
}

/** Tempo and key of a loaded track, after a correction or its analysis. */
export type TrackAnalysis = Pick<TrackInfo, 'tempo' | 'key' | 'gridEdited' | 'keyEdited'>;

/** A correction of the beat grid (see `edit_grid`). */
export type GridChange =
  | { kind: 'bpm'; bpm: number }
  | { kind: 'scale'; factor: number }
  | { kind: 'align' }
  | { kind: 'reset' };

/** A deck's settings, as the backend keeps them and the status stream tells them. */
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
  /** The record's own: where its track starts at its speed (s of music). */
  recordLeadInS: number;
  /** The turntable's left and right channels are crossed. */
  swapChannels: boolean;
  /** The record on its turntable (a timecode format's id). */
  timecode: string;
  /** The pitch stays when the speed changes. */
  keyLock: boolean;
  /** Software mixer: the channel fader raised from closed plays the deck, closed it stops it
   * on its cue point. */
  faderStart: boolean;
  /** The range of its tempo fader: its own, else the general one (0.08 for ±8 %). */
  tempoRange: number;
  strip: ChannelStrip;
  /** Its channel's knobs as the backend shows them. */
  knobs: StripKnobs;
  fx: FxChain;
  /** Name of the sound in each sampler slot. */
  samples: (string | null)[];
  samplerGain: number;
  /** Its size in beats: what its jumps go by, what its sized loop lasts. */
  sizeBeats: number;
  /** The deck plays its input as it comes instead of its track. */
  passthrough: boolean;
  /** Its side of the crossfader. */
  side: CrossfaderSide;
  /** A touch of its controller's jog wheel holds the record (else it only nudges). */
  jogScratch: boolean;
  /** At the end, the track goes on from its start. */
  repeat: boolean;
  /** The deck's channel silenced in the master, after its fader (the headphones still hear it). */
  mute: boolean;
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
  /** The effect's second knob (tone, resonance...: `PARAM2` of `effects.ts`), 0..1. */
  param2: number;
  /** The effect's period in beats. */
  beats: number;
}

/** A deck's beat effects, played one slot after the other. */
export interface FxChain {
  /** `FX_SLOTS` of them. */
  slots: FxSettings[];
  /** The whole chain on, each slot then as its own switch says; off, all silent. */
  enabled: boolean;
  /** The part of the track the chain is on, where separated; `null`: the whole deck. */
  part: StemPart | null;
}

export const FX_SLOTS = 3;

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

/** Smooth: equal power, for blending. Sharp: cuts only at the very ends, for scratching. */
export type CrossfaderCurve = 'smooth' | 'sharp';
/** The side of the crossfader a deck is on, or neither. */
export type CrossfaderSide = 'left' | 'center' | 'right';

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
  /** What it can come in on: the inputs the turntables and the other source leave. */
  choices: InputChannels[];
  /** Why it is not heard on the running device. */
  note: 'notOnDevice' | 'turntablesTakeAll' | null;
  /** Heard in the master. */
  on: boolean;
  /** Heard in the headphones. */
  cue: boolean;
}

/** Settings as the backend applies them. */
/** The knobs of a deck's channel: trim, EQ (low, mid, high), filter. */
export interface StripKnobs {
  trim: Knob;
  eq: [Knob, Knob, Knob];
  filter: Knob;
}

/** The knobs of the mixer beside its channels. */
export interface MixerKnobs {
  crossfader: number;
  cueMix: Knob;
  headphones: Knob;
  main: Knob;
  balance: Knob;
  booth: Knob;
  mic: Knob;
  aux: Knob;
}

export interface MixerState {
  decks: DeckSettings[];
  /** The knobs beside the channels, and where the crossfader stands (0 to 1). */
  knobs: MixerKnobs;
  /** From -1 (left) to 1. */
  crossfader: number;
  mode: MixMode;
  /** The software mixer is in use (or would be, the audio off). */
  internal: boolean;
  /** An external mixer chosen, the device too small for it: the outputs it needs. */
  fallback: number | null;
  /** The first output (0-based) each deck in use plays on to an external mixer. */
  deckOutputs: number[];
  headphones: HeadphonesOutput;
  /** First output (0-based) of the headphones in use; `null` when they are not heard. With an
   * external mixer, a pair no deck uses, for the preview alone. */
  headphonesInUse: number | null;
  /** The pairs the headphones can take, with the one chosen (first outputs). */
  headphonesChoices: number[];
  /** Where the headphones are heard on the running device. */
  headphonesNote: OutputNote | null;
  /** Headphones: the cue on the left and the master on the right, each in mono. */
  splitCue: boolean;
  crossfaderCurve: CrossfaderCurve;
  /** The crossfader's sides swapped: towards the left, the decks on the right come in. */
  crossfaderReverse: boolean;
  /** Master limiter. */
  limiter: boolean;
  /** First output (0-based) of the booth chosen, and of the booth in use. */
  booth: number | null;
  boothInUse: number | null;
  /** The pairs the booth can take: `taken` by the headphones, or free. */
  boothChoices: { first: number; taken: boolean }[];
  /** Where the booth chosen is heard, or why not, on the software mixer of the running device. */
  boothNote: OutputNote | null;
  mic: LiveState;
  aux: LiveState;
  /** While the microphone is on, the music ducks by `talkoverDepthDb`. */
  talkover: boolean;
  talkoverDepthDb: number;
  /** What was recorded last. */
  recordSource: RecordSource | null;
  /** What can be recorded on the running device: a turntable's inputs told apart. */
  recordOptions: { source: RecordSource; turntable: boolean }[];
}

/** Why an output chosen is heard or not on the running device. */
export type OutputNote =
  | { kind: 'on'; first: number }
  | { kind: 'notHeard' }
  | { kind: 'takenByHeadphones'; first: number }
  | { kind: 'notOnDevice' };

/** The decks there can be: two are shown, or all four (a preference). */
export const MAX_DECKS = 4;
