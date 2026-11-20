// What the status stream sends, as the backend declares it: written by its test
// (status_stream::types, PRISME_WRITE_TYPES=1), never by hand.

export type EngineStatus = {
  audioRunning: boolean;
  decks: Array<DeckStatus>;
  /**
   * A deck plays (see `view::plays`): no update is installed meanwhile.
   */
  plays: boolean;
  /**
   * What the meters show: each deck's, the master's, the live sources'.
   */
  meters: MeterLevels;
  health: AudioHealth | null;
  recording: RecordingStatus | null;
  preview: PreviewStatus | null;
  /**
   * The strip comparing two decks' beats.
   */
  phase: PhaseStrip;
  /**
   * Seconds before the Auto DJ's next mix starts, while one is planned.
   */
  mixInS: number | null;
};

/**
 * A deck as the engine has it and the screen shows it at a frame.
 */
export type DeckStatus = {
  loaded: boolean;
  playing: boolean;
  positionS: number;
  durationS: number;
  speed: number;
  /**
   * The rate it plays at: its tempo, gestures excluded, or its fader's without a track.
   */
  rate: number;
  /**
   * Its tempo, gestures excluded (needs a beat grid).
   */
  bpm: number | null;
  heardKey: MusicalKey | null;
  /**
   * The DJ hears or holds its track: a load would replace it.
   */
  plays: boolean;
  remainingS: number | null;
  /**
   * The end of its track near (on), or soon.
   */
  ending: LightState;
  /**
   * Who drives it.
   */
  mode: DeckMode;
  tempo: TempoFader;
  sync: SyncStatus;
  /**
   * Its loops, jumps and pads may move its track.
   */
  canMove: boolean;
  /**
   * Its motor (brake, spin back) may move its track.
   */
  canMotor: boolean;
  /**
   * Its grid may move onto the beats of the deck it is mixed with.
   */
  canAlignGrid: boolean;
  /**
   * Its key may be moved to mix with the one heard on the deck it is mixed with.
   */
  canSyncKey: boolean;
  lights: DeckLights;
  looping: LoopStatus;
  slipMode: boolean;
  /**
   * The reverse is on: the track plays backwards.
   */
  reverse: boolean;
  vinyl: VinylStatus;
  /**
   * Sampler slots playing (bit per slot).
   */
  samplesPlaying: number;
  /**
   * The parts of its separated track, a knob each.
   */
  stems: Array<StemKnob>;
  /**
   * How far its track's separation is, from 0 to 1: as it is separated, or as the downloads
   * of the models it waits for are.
   */
  separating: number | null;
};

export type MusicalKey = {
  /**
   * "Am", "F#"...
   */
  name: string;
  /**
   * "8A", "2B"...
   */
  camelot: string;
  /**
   * Its place on the Camelot wheel, 1 to 12.
   */
  number: number;
  minor: boolean;
};

/**
 * The state a light shows.
 */
export type LightState =
  | "on"
  | "off"
  | "paused"
  | "loaded"
  | "playing"
  | "soon";

/**
 * Who drives a deck.
 */
export type DeckMode = "int" | "relative" | "absolute" | "thru";

/**
 * A deck's tempo fader as shown.
 */
export type TempoFader = {
  /**
   * It does not set the tempo: a record does, or the sync.
   */
  held: boolean;
  /**
   * The tempo it shows, as a rate: the one played while held, else its own.
   */
  ratio: number;
  /**
   * Where it stands, 0 to 1.
   */
  position: number;
  /**
   * On a record the sync does not lock: the rate matching the deck it follows.
   */
  matching: number | null;
  /**
   * Where that rate is along the fader, within its range.
   */
  matchingAt: number | null;
};

export type SyncStatus = {
  state: SyncState;
  leader: boolean;
  /**
   * What its SYNC key does.
   */
  key: SyncKey;
};

/**
 * A deck's sync.
 */
export type SyncState = "off" | "matched" | "locked";

/**
 * What a deck's SYNC key does.
 */
export type SyncKey = "lock" | "once" | "noGrid" | "absolute";

/**
 * What a deck's keys and pads light by the controllers' rules.
 */
export type DeckLights = {
  /**
   * Its CUE: on at the cue point, paused stopped elsewhere.
   */
  cue: LightState;
  /**
   * What its pads on the screen show.
   */
  pads: PadsLights;
};

/**
 * What the pads of each page of a deck show.
 */
export type PadsLights = {
  loops: PageLights;
  roll: PageLights;
  jump: PageLights;
  slice: PageLights;
  cut: PageLights;
  fx: PageLights;
};

/**
 * What the pads of a page show, a bit per pad: lit by their action's light, and marked (a
 * loop or a jump of the deck's size, an effect sounding in another slot of the chain).
 */
export type PageLights = { lit: number; marked: number };

export type LoopStatus = {
  startS: number | null;
  endS: number | null;
  active: boolean;
  /**
   * Start marked by LOOP IN, waiting for LOOP OUT.
   */
  loopInS: number | null;
};

export type VinylStatus = {
  enabled: boolean;
  mode: VinylMode;
  /**
   * The needle reached the end of the record: the deck plays on by itself.
   */
  runOut: boolean;
  /**
   * The record has read backwards at play speed for a while: crossed channels?
   */
  crossed: boolean;
  signal: SignalState;
  /**
   * The signal as the deck's light shows it: none while no record drives it, none heard while
   * its channels seem crossed.
   */
  light: SignalState | null;
  /**
   * Timecode carrier level, as a share of a healthy line level's (0 to 1).
   */
  level: number;
  recordSpeed: number;
  recordPositionS: number | null;
};

export type VinylMode = "absolute" | "relative";

/**
 * The timecode as a deck hears it: none, a signal that comes and goes, a good one.
 */
export type SignalState = "none" | "bounce" | "ok";

/**
 * A part of a deck's separated track, as its knob shows it.
 */
export type StemKnob = {
  part: StemPart;
  level: number;
  on: boolean;
  /**
   * Its change waits for the next bar.
   */
  waiting: boolean;
  /**
   * Separated where the deck plays.
   */
  ready: boolean;
};

/**
 * A part of a track an effect can be on alone.
 */
export type StemPart = "vocals" | "drums" | "bass" | "other" | "instrumental";

/**
 * What each of the [`Meters`] shows.
 */
export type MeterLevels = {
  decks: [MeterLevel, MeterLevel, MeterLevel, MeterLevel];
  main: MeterLevel;
  mic: MeterLevel;
  aux: MeterLevel;
};

/**
 * What a meter shows, as positions on its scale: 0 at its floor or below, 1 at full scale.
 */
export type MeterLevel = {
  level: number;
  /**
   * The highest level of late.
   */
  held: number;
  /**
   * A peak reached full scale a moment ago.
   */
  clip: boolean;
};

/**
 * How well the audio keeps up.
 */
export type AudioHealth = {
  /**
   * Periods processed too late: each one is an audible click.
   */
  overBudget: number;
  worstCallbackUs: number;
  inputUnderruns: number;
  /**
   * Input frames dropped: more had come than the output took.
   */
  inputDropped: number;
  deviceLost: boolean;
  /**
   * The engine failed and could not be restarted: the audio is silent.
   */
  engineFailed: boolean;
  /**
   * Frames per period, as the driver calls the engine (0 before the first period).
   */
  periodFrames: number;
  /**
   * How long a period lasts, once the driver has called the engine.
   */
  periodMs: number | null;
};

export type RecordingStatus = {
  seconds: number;
  /**
   * Audio the writer could not take in time (lost).
   */
  droppedSeconds: number;
};

export type PreviewStatus = {
  id: number;
  positionS: number;
  /**
   * Estimated until the track is all read.
   */
  durationS: number;
  /**
   * Its audio is not read that far yet: it waits, silent.
   */
  waiting: boolean;
};

/**
 * The strip comparing two decks' beats.
 */
export type PhaseStrip =
  & {
    /**
     * The decks compared: the first two playing, else A and B.
     */
    decks: [number, number];
  }
  & (
    | { "state": "off" }
    | { "state": "drifting"; seconds: number }
    | { "state": "matched" }
    | { "state": "ahead"; ms: number }
    | { "state": "behind"; ms: number }
  );

/**
 * What the strip tells of the second deck's beats against the first's.
 */
export type PhaseReading =
  | { "state": "off" }
  | { "state": "drifting"; seconds: number }
  | { "state": "matched" }
  | { "state": "ahead"; ms: number }
  | { "state": "behind"; ms: number };
