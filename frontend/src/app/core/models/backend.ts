// What the backend sends the page and takes from it, as it declares it: written by
// its test (ts_types, PRISME_WRITE_TYPES=1), never by hand.

/**
 * How far the analysis of the music folders is.
 */
export type AnalysisProgress = {
  total: number;
  done: number;
  failed: number;
  running: boolean;
  /**
   * Held back while a deck plays.
   */
  paused: boolean;
};

/**
 * The APK of a release, and its size in bytes.
 */
export type Apk = { url: string; size: number };

export type AudioBackend =
  | "wasapi"
  | "asio"
  | "coreAudio"
  | "alsa"
  | "jack"
  | "aaudio"
  | "usb";

/**
 * A change of the audio settings, drafted in the settings window until applied.
 */
export type AudioChange =
  | { "setting": "backend"; "value": AudioBackend }
  | { "setting": "driver"; "value": DeviceChoice | null }
  | { "setting": "output"; "value": DeviceChoice | null }
  | { "setting": "input"; "value": DeviceChoice | null }
  | { "setting": "turntablesOnDriver"; "value": boolean }
  | { "setting": "buffer"; "value": number | null }
  | { "setting": "simulate"; "value": boolean };

export type AudioDevice = {
  /**
   * What its system calls it, by which it is chosen ([`DeviceChoice`]).
   */
  id: string;
  name: string;
  /**
   * The sound card it is a side of: an interface's outputs and inputs share it.
   */
  card: string;
  /**
   * "input" or "output"
   */
  kind: string;
  channels: number;
  sampleRate: number;
  /**
   * The periods the settings offer that the driver accepts.
   */
  bufferSizes: Array<BufferSize>;
  isDefault: boolean;
};

/**
 * The audio settings drafted in the settings, as the backend shapes them.
 */
export type AudioDraft = {
  draft: AudioSettings;
  /**
   * It differs from the settings kept: Apply restarts the audio.
   */
  changed: boolean;
  /**
   * The periods offered: the output chosen's, else the default output's, else every one at
   * 48 kHz.
   */
  bufferSizes: Array<BufferSize>;
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

/**
 * The audio running, as the settings show it.
 */
export type AudioInfo = {
  /**
   * Turntables can drive the decks: the audio reads their timecode from its inputs, or
   * simulates it.
   */
  turntables: boolean;
  backend: AudioBackend;
  device: string;
  sampleRate: number;
  outputChannels: number;
  inputChannels: number;
  /**
   * Decks of the engine.
   */
  decks: number;
  simulated: boolean;
  /**
   * The sound card chosen in the settings while it is missing and the audio plays on the
   * default devices meanwhile (empty: the first one a driver finds).
   */
  missing: string | null;
};

/**
 * Audio settings chosen by the user.
 */
export type AudioSettings = {
  backend: AudioBackend;
  output: DeviceChoice | null;
  /**
   * Turntable input (DVS interface). None = no turntables.
   */
  input: DeviceChoice | null;
  /**
   * Feed deck 1 with a simulated turntable instead of the input.
   */
  simulate: boolean;
  bufferFrames: number | null;
};

/**
 * The audio as the settings show it.
 */
export type AudioState = {
  /**
   * The settings the DJ chose: the audio runs on the default devices while one of theirs is
   * missing.
   */
  settings: AudioSettings;
  /**
   * The audio running, if it does.
   */
  info: AudioInfo | null;
  /**
   * The USB card the DJ is asked to let Prisme open, or refused it.
   */
  asked: CardAsked | null;
  /**
   * The default output another application held as the audio started.
   */
  busy: string | null;
};

/**
 * Why the Auto DJ stopped, paused or left a track out.
 */
export type AutoDjReason =
  | "externalMixer"
  | "records"
  | "listEmpty"
  | "listEnded"
  | "transport"
  | "crossfader"
  | "load"
  | "unloadable";

export type AutoDjState = "off" | "on" | "mixing" | "paused";

/**
 * The Auto DJ as the interface shows it.
 */
export type AutoDjStatus = {
  state: AutoDjState;
  reason: AutoDjReason | null;
  reasonDeck: number | null;
  /**
   * The deck being mixed in.
   */
  incoming: number | null;
  fadeBeats: number;
  fadeS: number;
};

/**
 * An audio driver the settings offer.
 */
export type BackendChoice = {
  backend: AudioBackend;
  /**
   * One device for the outputs and, with turntables on it, the inputs (an ASIO driver, a USB
   * card Prisme drives itself).
   */
  oneDevice: boolean;
};

/**
 * An error as the interface is sent it.
 */
export type BackendError = {
  code: string;
  /**
   * What the interface needs to word it.
   */
  params: Record<string, string | number | boolean | null>;
  /**
   * Its English message.
   */
  message: string;
};

export type Band = "low" | "mid" | "high";

/**
 * A pair of outputs the booth can be on: heard there, or `taken` by the headphones.
 */
export type BoothChoice = { first: number; taken: boolean };

/**
 * A period of the audio: so many frames, so long at the device's rate.
 */
export type BufferSize = { frames: number; ms: number };

/**
 * A USB card the DJ is asked to let Prisme open (Android asks), or refused it.
 */
export type CardAsked = { card: string; refused: boolean };

/**
 * A sound card just plugged in, offered to the DJ with the settings that use it.
 */
export type CardOffer = {
  /**
   * A controller plugged in with no mixer of its own, that Prisme mixes for: its card is
   * likely this one.
   */
  controller: string | null;
  /**
   * The card, as its system tells it apart: what the DJ declines.
   */
  card: string;
  name: string;
  settings: AudioSettings;
};

/**
 * A channel of the software mixer, before its fader. Gains are linear.
 */
export type ChannelStrip = {
  trim: number;
  /**
   * Low, mid, high.
   */
  eq: [number, number, number];
  kills: [boolean, boolean, boolean];
  /**
   * -1 = low-pass closed, 0 = off, 1 = high-pass open.
   */
  filter: number;
};

/**
 * Another action for a key pressed while the key `with` is held.
 */
export type Chord = { with: PhysicalControl; target: string };

/**
 * What the interface mirrors of the decks, their tracks, the mixer and the DJ's preferences,
 * by section (`state::Section`): those that did not change since last sent are left out.
 */
export type ControlChanges = {
  mixer?: MixerState;
  routing?: RoutingState;
  /**
   * The decks' settings.
   */
  decks?: Array<DeckSettings>;
  quantize?: boolean;
  /**
   * The library over the whole window.
   */
  libraryOnly?: boolean;
  /**
   * The range of the tempo faders without one of their own.
   */
  pitchRange?: number;
  /**
   * Each deck's track, as it is now.
   */
  tracks?: Array<TrackInfo | null>;
  /**
   * Whether each deck loads a track.
   */
  loading?: [boolean, boolean, boolean, boolean];
  /**
   * What each deck's pads hold.
   */
  perform?: [DeckPerform, DeckPerform, DeckPerform, DeckPerform];
  preview?: PreviewState;
  /**
   * The list browsed, and the rows of its window.
   */
  library?: LibraryView;
  lists?: LibraryLists;
  preferences?: Preferences;
  /**
   * The separation into stems, with the DJ's preferences of it.
   */
  stems?: StemsState;
  controllers?: ControllersState;
  audio?: AudioState;
  folders?: FoldersState;
  autoDj?: AutoDjStatus;
};

/**
 * How a control drives an action: a key (pressed and let go, held or not), a fader over its
 * travel, an encoder by its steps, or a light that only shows a state.
 */
export type ControlKind = "button" | "fader" | "encoder" | "light";

/**
 * A controller, MIDI or HID.
 */
export type Controller = {
  /**
   * Its MIDI port's name, or the name Prisme gives a HID device.
   */
  name: string;
  enabled: boolean;
  plugged: boolean;
  /**
   * The model Prisme knows it as, its controls mapped by its profile.
   */
  profile: string | null;
  /**
   * The file of its profile.
   */
  profileFile: string | null;
  /**
   * The profiles the DJ may choose for it: its model's, then the variants of it.
   */
  offered: Array<OfferedProfile>;
  /**
   * Its profile was tried on the controller itself, not only read from another program's.
   */
  verified: boolean;
  /**
   * Where its profile comes from.
   */
  profileSource: ProfileSource | null;
  /**
   * Prisme's model that the DJ's own profile takes the place of on it.
   */
  replaces: string | null;
  /**
   * The file its profile is exported as, by default.
   */
  exportFile: string;
  /**
   * Listed with the controllers: a MIDI port, a HID device of a model Prisme knows, or one in
   * use (most other HID devices are no controllers).
   */
  listed: boolean;
  /**
   * What it drives can be written as a profile: it is in use and plugged in.
   */
  canExport: boolean;
};

/**
 * A control of a controller driving one of the application's actions.
 */
export type ControllerMapping = {
  /**
   * The controller's name: its MIDI port's, or Prisme's of a HID device.
   */
  device: string;
  control: PhysicalControl;
  target: string;
  /**
   * What the control is called on the controller, as printed there (`"STORE"`): the
   * interface names it so.
   */
  name?: string | null;
  encoding?: Encoding;
  /**
   * The controller sending the low 7 bits of a 14-bit control (its high 7 bits on
   * `control`, a controller, by convention the number 32 lower).
   */
  fine?: number | null;
  /**
   * Drives its action while the controller's shift is held (its second layer), or a
   * named shift.
   */
  shift?: Shift;
  /**
   * Drives its action only while its group is in this mode.
   */
  when?: When | null;
  /**
   * A fader whose lowest value is at its top.
   */
  invert?: boolean;
  /**
   * What its light shows in each state of its action, when it is not 127 and 0.
   */
  light?: LightValues;
  /**
   * Drives its action, as a key pressed, when the control sends this value (a switch in
   * one of its positions).
   */
  onValue?: number | null;
  /**
   * Other actions it drives along with its own.
   */
  also?: Array<string>;
  /**
   * Another action for the key held down a while.
   */
  hold?: Hold | null;
  /**
   * Another action for the key pressed twice within `ms`.
   */
  double?: Hold | null;
  /**
   * Another action for the key pressed while another is held.
   */
  chord?: Chord | null;
  /**
   * Another action for a shift key pressed and let go alone, no other control of its
   * controller moved meanwhile.
   */
  alone?: string | null;
  /**
   * An encoder driving a key's action: a step to the right presses `target`, a step to the
   * left this one.
   */
  down?: string | null;
  /**
   * An encoder's steps multiplied by this; below 1 they add up to whole steps.
   */
  scale?: number;
  /**
   * How far a step of an encoder moves a fader's action, a share of its travel.
   */
  step?: number | null;
  /**
   * A key putting a fader's action at a position, or at each of two in turn (lit at the
   * second).
   */
  set?: FaderSet | null;
  /**
   * A key driving an action while held, latched: a press holds it, the next lets it go.
   */
  latch?: boolean;
  /**
   * A mode key choosing its mode while held: let go, its group is back in the mode before.
   */
  held?: boolean;
  /**
   * A mode key choosing its mode for one action of its group, which then goes back to its
   * first mode.
   */
  once?: boolean;
  /**
   * Its light's values by the colour of its action (a hot cue's, `"#ff0000": 5`): the
   * nearest colour's.
   */
  palette?: { [key in string]: number };
};

export type ControllerProfile = {
  file: string;
  name: string;
  source: ProfileSource;
  verified: boolean;
  /**
   * Prisme's model whose profile of the same file name this one, the DJ's own, replaces.
   */
  replaces: string | null;
};

/**
 * The controller models Prisme knows, and where the DJ's own profiles go.
 */
export type ControllerProfiles = {
  profiles: Array<ControllerProfile>;
  /**
   * The models known (their variants aside): all of them, those updated since, the DJ's own.
   */
  counts: ProfileCounts;
  /**
   * The DJ's own profiles that take the place of Prisme's of the same file name.
   */
  replacing: Array<OfferedProfile>;
  /**
   * The DJ's files that could not be read, and why (`file: why`).
   */
  unreadable: Array<string>;
  /**
   * The folder of the DJ's own profiles, if Prisme has a data folder.
   */
  folder: string | null;
};

/**
 * The controllers as the interface shows them.
 */
export type ControllersState = {
  /**
   * The controller models Prisme knows.
   */
  profiles: ControllerProfiles;
  /**
   * What drives what, by action and by controller.
   */
  mapped: Array<Mapped>;
  /**
   * Where the MIDI clock goes.
   */
  clockOutput: string | null;
  /**
   * The action the next control moved will drive.
   */
  learning: string | null;
  controllers: Array<Controller>;
  /**
   * A controller is open: controls can be learnt.
   */
  canLearn: boolean;
};

export type Crate = {
  id: number;
  name: string;
  /**
   * The rules of a smart crate.
   */
  rules: SmartRules | null;
  trackCount: number;
};

export type CrossfaderCurve = "smooth" | "sharp";

export type CrossfaderSide = "left" | "center" | "right";

export type Cue = {
  /**
   * Hot cue slot, `None` for the main cue.
   */
  hot: number | null;
  positionS: number;
  loopLengthS: number | null;
  /**
   * 0xRRGGBB
   */
  color: number;
  label: string;
};

/**
 * Two decks, or four. Saved as the number, as the interface has it: any other than four
 * reads as two.
 */
export type DeckCount = number;

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

export type DeckMark = { deck: number; playing: boolean };

/**
 * Who drives a deck.
 */
export type DeckMode = "int" | "relative" | "absolute" | "thru";

/**
 * Which slots of a deck's effect chain its pads and knobs act on.
 */
export type DeckPerform = {
  /**
   * The slot of its effect chain the effect knobs turn.
   */
  fxFocus: number | null;
  /**
   * The slot of its effect chain the screen's pads and knobs act on.
   */
  fxShown: number;
};

export type DeckSettings = {
  pitch: number;
  volume: number;
  cue: boolean;
  vinylEnabled: boolean;
  vinylMode: VinylMode;
  rpm: Rpm;
  /**
   * Chosen, else the record's own.
   */
  leadInS: number | null;
  /**
   * The record's own: where its track starts at its speed (s of music).
   */
  recordLeadInS: number;
  swapChannels: boolean;
  /**
   * The record on its turntable (a timecode format's id).
   */
  timecode: string;
  keyLock: boolean;
  /**
   * Software mixer: the channel fader starts and stops the deck.
   */
  faderStart: boolean;
  /**
   * The range of its tempo fader: its own, else the general one.
   */
  tempoRange: number;
  strip: ChannelStrip;
  /**
   * Its channel's knobs, as the screen shows them.
   */
  knobs: StripKnobs;
  fx: FxChain;
  /**
   * Name of the sound in each sampler slot.
   */
  samples: Array<string | null>;
  samplerGain: number;
  /**
   * Its size in beats: what its jumps go by, what its sized loop lasts.
   */
  sizeBeats: number;
  /**
   * The deck plays its input instead of its track.
   */
  passthrough: boolean;
  /**
   * Records may drive it: turntables can be read on the device, or it is handed to one.
   */
  records: boolean;
  /**
   * The first of the input pair its turntable and its passthrough come in on (0-based).
   */
  input: number | null;
  /**
   * The deck whose turntable it shares and reads the record of, on a device without a pair of
   * inputs for each deck.
   */
  layeredOn: number | null;
  /**
   * Its side of the crossfader.
   */
  side: CrossfaderSide;
  /**
   * At the end, the track goes on from its start.
   */
  repeat: boolean;
  /**
   * Silenced in the master, after its fader (still heard in the headphones).
   */
  mute: boolean;
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

/**
 * How the decks are laid out in their own window.
 */
export type DecksLayout = "auto" | "stage" | "lanes";

/**
 * A device chosen in the settings: the id its system gives it, by which it is found again, and
 * its name, shown, and by which the same model is found when its id is not there (see
 * [`DeviceRef`]).
 */
export type DeviceChoice = { id: string; name: string };

export type DeviceMappings = {
  device: string;
  mappings: Array<ControllerMapping>;
};

/**
 * A folder looked into to add it to the music folders: the folders in it, and how many tracks.
 */
export type DirListing = {
  path: string;
  /**
   * The music folder it is or is in, the deepest.
   */
  within: string | null;
  parent: string | null;
  folders: Array<Folder>;
  tracks: number;
};

export type DjPreferences = {
  vinylSync: VinylSyncMode;
  quantize: boolean;
  /**
   * Range of the tempo faders of the decks without one of their own (0.08 = ±8 %).
   */
  pitchRange: number;
  /**
   * Every track played at the same loudness (see `LoadedTrack::level`).
   */
  autoGain: boolean;
  deckCount: DeckCount;
  /**
   * The knobs with a middle (a mark at their top) hold there when turned through it.
   */
  knobMagnet: boolean;
  /**
   * On a phone, the decks play and pause from its lock screen and its notifications.
   */
  lockControls: boolean;
  /**
   * The headphones go to the deck a track is loaded on, alone.
   */
  cueOnLoad: boolean;
  /**
   * A load onto a deck that plays is refused, rather than asked about.
   */
  protectPlaying: boolean;
};

/**
 * Files and folders dropped from the computer's file manager: the tracks Prisme plays, the
 * rest taken for folders.
 */
export type Dropped = { tracks: Array<string>; folders: Array<string> };

export type EffectKind =
  | "echo"
  | "pingPong"
  | "reverb"
  | "flanger"
  | "phaser"
  | "filter"
  | "gate"
  | "bitcrusher";

/**
 * How a control's values are read. The steps are told here in 7-bit values; a HID field's
 * are read the same over its own bits (a 4-bit counter wraps at 16).
 */
export type Encoding =
  | "absolute"
  | "relative"
  | "relativeOffset"
  | "relativeOffsetNoZero"
  | "relativeSigned"
  | "relativeZero"
  | "absoluteWrap";

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
 * A list of an export to rekordbox, and where it goes there.
 */
export type ExportList = {
  list: ExportedList;
  folder: string | null;
  name: string;
};

/**
 * What an export to rekordbox wrote.
 */
export type ExportSummary = { tracks: number; lists: number };

/**
 * A list of the library: a crate (a smart one with the tracks its rules pick now), the
 * prepare list or a set of the history.
 */
export type ExportedList = { "kind": "crate"; id: number } | {
  "kind": "prepare";
} | { "kind": "session"; id: number };

/**
 * Where a key puts a fader's action: at a position, or at each of two in turn.
 */
export type FaderSet = number | [number, number];

/**
 * A fader of the screen that reads nothing under it.
 */
export type FaderState = {
  /**
   * Where it stands, from 0 to 1.
   */
  at: number;
  /**
   * Where a double click puts it back.
   */
  home: number;
};

export type Folder = { name: string; path: string };

/**
 * The folder shown: the music folder it is in, the folder above inside it, and the folders in
 * it.
 */
export type FolderView = {
  path: string;
  root: string | null;
  up: string | null;
  folders: Array<Folder>;
};

/**
 * The music folders, and how far their analysis is.
 */
export type FoldersState = {
  /**
   * The main one first.
   */
  folders: Array<MusicFolder>;
  progress: AnalysisProgress;
  /**
   * A rescan the DJ asked for runs.
   */
  rescanning: boolean;
  /**
   * How many rescans the DJ asked for ended: the library is up to date as one does.
   */
  rescans: number;
  /**
   * The library's database could not be opened: what changes is lost at exit.
   */
  inMemory: boolean;
};

/**
 * A deck's beat effects, in series.
 */
export type FxChain = {
  slots: [FxSettings, FxSettings, FxSettings];
  /**
   * The whole chain on, each slot then as its own switch says; off, all silent.
   */
  enabled: boolean;
  /**
   * The part of the track the chain is on, where separated; none: the whole deck.
   */
  part: StemPart | null;
};

/**
 * A slot of a deck's effect chain.
 */
export type FxSettings = {
  kind: EffectKind;
  enabled: boolean;
  /**
   * Level of the effect, 0..1.
   */
  mix: number;
  /**
   * The effect's main knob, 0..1.
   */
  amount: number;
  /**
   * The effect's second knob (tone, resonance...), 0..1; in the middle, the effect sounds as
   * it did without one.
   */
  param2: number;
  /**
   * The effect's period in beats.
   */
  beats: number;
};

/**
 * A beat, counted from bar 1's first, and where it falls.
 */
export type GridAnchor = { beat: number; timeS: number };

export type GridChange =
  | { "kind": "bpm"; bpm: number }
  | { "kind": "scale"; factor: number }
  | { "kind": "shift"; seconds: number }
  | { "kind": "beatHere" }
  | { "kind": "barHere" }
  | { "kind": "pinHere" }
  | { "kind": "unpin" }
  | { "kind": "align" }
  | { "kind": "reset" };

/**
 * Where the headphones go: those of the software mixer, or with an external mixer those of
 * the preview.
 */
export type HeadphonesOutput = { "kind": "auto" } | { "kind": "off" } | {
  "kind": "pair";
  first: number;
};

/**
 * A value in a report: a key's bit, a fader's 12 bits, a counter's byte, a light's.
 */
export type HidField = {
  /**
   * The id of its report, 0 for a device that does not number its reports.
   */
  report?: number;
  /**
   * Its first byte, counted from the report's first.
   */
  byte: number;
  /**
   * Its lowest bit in that byte.
   */
  bit?: number;
  /**
   * How many bits it has: 1 for a key or a light, up to 16; 24 for a light of three colours
   * (its red, green and blue bytes).
   */
  bits?: number;
  /**
   * Its bytes from the most significant, when it has several (little-endian otherwise, as
   * most devices write them).
   */
  bigEndian?: boolean;
};

export type HistorySession = {
  id: number;
  startedAtMs: number;
  playCount: number;
};

/**
 * A second action for a key held down for `ms` milliseconds: pressed and let go sooner, it
 * drives its own.
 */
export type Hold = { ms: number; target: string };

/**
 * What an import brought.
 */
export type ImportSummary = {
  tracks: number;
  cues: number;
  grids: number;
  crates: number;
  missing: number;
};

/**
 * The input channels a live source comes in on: one, heard in the middle, or a stereo pair.
 */
export type InputChannels = {
  /**
   * The first (0-based).
   */
  first: number;
  stereo: boolean;
};

/**
 * A key of the keyboard (`KeyboardEvent.code`, so the layout does not matter), with the
 * modifiers held: Control is Command on a Mac.
 */
export type Key = { code: string; ctrl?: boolean; shift?: boolean };

/**
 * A knob of the screen.
 */
export type KnobState = {
  /**
   * Where it stands, from 0 to 1.
   */
  at: number;
  reads: Readout;
  /**
   * Where a double click puts it back: unity gain, or the centre.
   */
  home: number;
};

export type Label = { key: string; params?: Record<string, LabelParam> };

/**
 * What a placeholder of a label is filled with: a number, a text shown as it is (a deck's
 * letter, a fraction of a beat), or another label, translated too.
 */
export type LabelParam = number | string | Label;

/**
 * A round trip measured from an output to an input.
 */
export type Latency = { roundTripMs: number; sampleRate: number };

/**
 * The channels a latency measurement may join, counted from 1.
 */
export type LatencyChannels = { outputs: number; inputs: number };

export type LibraryFocus = "tracks" | "lists";

/**
 * The lists the library keeps, as the side panel shows them.
 */
export type LibraryLists = {
  crates: Array<Crate>;
  sessions: Array<HistorySession>;
  /**
   * How many tracks the prepare list holds, and the first: the one the Auto DJ mixes in next.
   */
  prepare: number;
  next: NamedTrack | null;
  /**
   * How many tracks of the library are under none of the music folders.
   */
  elsewhere: number;
};

/**
 * What the menu of tracks can do with them: the selection's, or a row's.
 */
export type LibraryMenu = {
  /**
   * How many tracks it acts on, and the first.
   */
  count: number;
  first: NamedTrack | null;
  /**
   * They can be taken out of the list shown, this one.
   */
  removable: TrackSource | null;
  /**
   * Those of them under none of the music folders: all of them, or none.
   */
  elsewhere: number;
  /**
   * The folder they share, all from elsewhere, which may join the music folders unless it
   * holds one (`holds`).
   */
  folder: string | null;
  holds: string | null;
  /**
   * The genre a genre given to them starts with: the one they share, else their folder's name.
   */
  genre: string;
  /**
   * How many the installed models have stems to add to.
   */
  separable: number;
};

export type LibraryPreferences = {
  /**
   * The music folder chosen, else the system's (see `folders::music_root`).
   */
  musicFolder: string | null;
  /**
   * The music folders added next to it (a USB key, a drive, a sets folder), in the order
   * they were added. None on a phone.
   */
  folders: Array<string>;
  lastFolder: string | null;
  /**
   * Columns of the track list the DJ hid (frontend ids).
   */
  hiddenColumns: Array<string>;
  /**
   * Tempo and key written into the files' tags (see `tag_writer`).
   */
  writeTags: boolean;
  /**
   * Those another program wrote (Mixed In Key, say) replaced as well.
   */
  replaceTags: boolean;
  /**
   * The background analysis of the music folder waits while a deck plays.
   */
  pauseWhilePlaying: boolean;
  /**
   * A track loaded on a deck is separated right away, before the library's; one put in
   * the prepare list in the background. Not chosen: with a graphics card only (see
   * `stems_service::separates_on_load`).
   */
  stemsOnLoad: boolean | null;
  /**
   * The separation runs on the graphics chip when there is one worth it (else on the
   * processor alone, minutes a track).
   */
  stemsGpu: boolean;
  /**
   * The small separation models, faster, rather than the large ones (a phone's default).
   */
  stemsFast: boolean;
};

/**
 * A row of the library as it is drawn: its track, what the library knows of it, and how it
 * goes with the rest of the application.
 */
export type LibraryRow = {
  path: string;
  /**
   * When it was played, in a set: a track played twice is two rows.
   */
  playedAtMs: number | null;
  /**
   * Its place in its list, from 1, in a list with an order of its own.
   */
  position: number | null;
  /**
   * Its title tag, else its file name without the extension.
   */
  title: string;
  artist: string | null;
  album: string | null;
  genre: string | null;
  year: number | null;
  bpm: number | null;
  key: MusicalKey | null;
  /**
   * 0xRRGGBB
   */
  color: number | null;
  /**
   * 0 (not rated) to 5.
   */
  rating: number;
  plays: number;
  /**
   * Its analysis is known (until then its values are waited for).
   */
  analysed: boolean;
  /**
   * Its bars are to be checked by ear (`prisme_analysis::Tempo::bars_sure`), its grid not
   * corrected by hand.
   */
  barsToCheck: boolean;
  /**
   * Its file could not be decoded.
   */
  failed: boolean;
  stems: RowStems;
  selected: boolean;
  /**
   * Played in the set going on.
   */
  played: boolean;
  /**
   * Under none of the music folders.
   */
  elsewhere: boolean;
  /**
   * Found by the search near what was typed, a slip of the finger away: after those found
   * as typed.
   */
  near: boolean;
  /**
   * In the headphones' preview, or about to be.
   */
  previewing: boolean;
  /**
   * The decks holding it.
   */
  decks: Array<DeckMark>;
  /**
   * Its key mixes with the one heard on the deck the tracks are compared with.
   */
  fits: boolean;
  /**
   * The pitch that brings it to that deck's tempo, in percent, to a tenth.
   */
  pitch: number | null;
  /**
   * Whether the tempo fader reaches it.
   */
  reachable: boolean;
};

export type LibrarySort = { column: SortColumn; descending: boolean };

/**
 * The library as the interface shows it: the list browsed and the rows it draws, those of the
 * window it asked for (`library_viewport`).
 */
export type LibraryView = {
  /**
   * Moved by every change of what is shown and of the window: the highest is the latest.
   */
  seq: number;
  source: TrackSource;
  /**
   * The folder shown.
   */
  folder: FolderView | null;
  scope: SearchScope;
  /**
   * The search applied.
   */
  filter: string;
  sort: LibrarySort;
  /**
   * The list has an order of its own, shown in its rows.
   */
  ordered: boolean;
  /**
   * Its rows can be dragged to another place.
   */
  reorderable: boolean;
  /**
   * How many rows are shown.
   */
  count: number;
  /**
   * Where the cursor is among the rows shown.
   */
  cursor: number | null;
  /**
   * Moved each time the cursor goes to another row (not when rows before it move).
   */
  cursorMoves: number;
  /**
   * Moved each time another list is shown, or the list is searched, sorted or scoped.
   */
  listed: number;
  /**
   * How many tracks the actions on the selection take.
   */
  selected: number;
  focus: LibraryFocus;
  pointed: Shelf;
  /**
   * The deck the tracks are compared with.
   */
  reference: number;
  /**
   * Why the music folder shown at start could not be read.
   */
  unread: BackendError | null;
  window: LibraryWindow;
};

/**
 * The rows of the window, from its `first`.
 */
export type LibraryWindow = {
  seq: number;
  first: number;
  rows: Array<LibraryRow>;
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
 * A light's value in a state: steady, two in turn, or a whole message of its own (for the
 * models addressing their lights otherwise than by a value on the control's note; of a HID
 * light, its bytes: the red, green and blue of a light of three colours).
 */
export type LightValue =
  | number
  | { blink: [number, number]; ms: number }
  | Array<number>;

/**
 * What a light shows for each state of its action: `on` and `off`, or the action's own
 * (`loaded`, `playing`…). A state it does not name shows as `on`, but `off`: 127 and 0
 * when it names neither.
 */
export type LightValues = { [key in string]: LightValue };

/**
 * Why a live source is not heard on the running device.
 */
export type LiveNote = "notOnDevice" | "turntablesTakeAll";

/**
 * A live source of the software mixer.
 */
export type LiveSource = "mic" | "aux";

/**
 * A live source of the software mixer as the backend knows it.
 */
export type LiveState = {
  /**
   * The input channels chosen.
   */
  input: InputChannels | null;
  /**
   * They are on the running device, left free by the turntables.
   */
  inUse: boolean;
  /**
   * What it can come in on: the inputs the turntables and the other source leave.
   */
  choices: Array<InputChannels>;
  /**
   * Why it is not heard on the running device.
   */
  note: LiveNote | null;
  on: boolean;
  cue: boolean;
};

export type LoopStatus = {
  startS: number | null;
  endS: number | null;
  active: boolean;
  /**
   * Start marked by LOOP IN, waiting for LOOP OUT.
   */
  loopInS: number | null;
};

/**
 * An action the controllers in use drive, and what drives it on each.
 */
export type Mapped = { target: Target; devices: Array<DeviceMappings> };

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
 * What each of the [`Meters`] shows.
 */
export type MeterLevels = {
  decks: [MeterLevel, MeterLevel, MeterLevel, MeterLevel];
  main: MeterLevel;
  mic: MeterLevel;
  aux: MeterLevel;
};

/**
 * Where a MIDI message goes.
 */
export type Midi = {
  /**
   * 0 to 15.
   */
  channel: number;
  /**
   * Note or controller number; 0 for the pitch bend.
   */
  number: number;
};

export type MixMode = "external" | "internal";

/**
 * The knobs of the mixer beside its channels, and its crossfader.
 */
export type MixerKnobs = {
  /**
   * The crossfader, from 0 (left) to 1, back in the middle.
   */
  crossfader: FaderState;
  cueMix: KnobState;
  headphones: KnobState;
  main: KnobState;
  balance: KnobState;
  booth: KnobState;
  mic: KnobState;
  aux: KnobState;
};

/**
 * The mixer as the interface shows it: its knobs, the DJ's preferences of it.
 */
export type MixerState = {
  /**
   * The knobs beside the channels and the crossfader, as the screen shows them.
   */
  knobs: MixerKnobs;
  /**
   * Headphones: the cue on the left and the master on the right.
   */
  splitCue: boolean;
  crossfaderCurve: CrossfaderCurve;
  /**
   * The crossfader's sides swapped.
   */
  crossfaderReverse: boolean;
  limiter: boolean;
  talkover: boolean;
  talkoverDepthDb: number;
  /**
   * The last recording ended.
   */
  lastRecording: Recording | null;
};

/**
 * A choice of models: what it has left to download, and why its download failed.
 */
export type ModelChoice = {
  /**
   * `None` once it is installed.
   */
  bytesLeft: number | null;
  error: string | null;
};

/**
 * A separation model: the vocals one, the drums and bass one, and a small one, faster.
 */
export type ModelId = "vocals" | "drums-bass" | "four-fast";

export type ModelRow = {
  /**
   * It can be downloaded now: the drums and bass come with the vocals only.
   */
  ready: boolean;
  id: ModelId;
  installed: boolean;
  downloading: number | null;
  error: string | null;
  bytes: number;
};

/**
 * A music folder: the main one, or one added next to it (a USB key, a drive, a sets
 * folder).
 */
export type MusicFolder = {
  path: string;
  main: boolean;
  /**
   * The folder is there now: one on a drive unplugged is not, its tracks kept.
   */
  present: boolean;
};

export type MusicalKey = {
  /**
   * "Am", "F#"...
   */
  name: string;
  /**
   * Its tonic's letter, "C" to "B".
   */
  note: string;
  /**
   * Its tonic's "♯" or "♭", or nothing.
   */
  accidental: string;
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
 * A track by its path, its title, and what tells it apart in a list: its tempo, its key.
 */
export type NamedTrack = {
  path: string;
  title: string;
  artist: string | null;
  bpm: number | null;
  key: MusicalKey | null;
};

/**
 * A newer version than the one running, its APK handed to Android.
 */
export type NewVersion = { version: string; apk: Apk };

/**
 * A profile the DJ may choose for a controller.
 */
export type OfferedProfile = { file: string; name: string };

/**
 * A controller in use opened again: its device, and the model it is.
 */
export type Opened = { device: string; model: string };

/**
 * Why an output chosen is heard or not on the running device.
 */
export type OutputNote =
  | { "kind": "on"; first: number }
  | { "kind": "notHeard" }
  | { "kind": "takenByHeadphones"; first: number }
  | { "kind": "notOnDevice" };

export type PadName = {
  /**
   * Its action, of whichever deck (`loop.0.25`).
   */
  action: string;
  /**
   * What it shows: its beats, or its number.
   */
  caption: string;
  label: Label;
  /**
   * The effect an effects' pad turns on, on the slot shown.
   */
  effect: EffectKind | null;
};

/**
 * A page of a deck's pads on the screen whose pads each perform an action of the deck.
 */
export type PadPage = "loops" | "roll" | "jump" | "slice" | "cut" | "fx";

/**
 * A page of a deck's pads on the screen.
 */
export type PadPageNames = { page: PadPage; pads: Array<PadName> };

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

/**
 * What the strip tells of the second deck's beats against the first's.
 */
export type PhaseReading =
  | { "state": "off" }
  | { "state": "drifting"; seconds: number }
  | { "state": "matched" }
  | { "state": "ahead"; ms: number }
  | { "state": "behind"; ms: number };

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
 * A physical control of a controller: what one of its MIDI messages addresses, or a field of
 * one of its HID reports.
 */
export type PhysicalControl =
  | { "kind": "note" } & Midi
  | { "kind": "controlChange" } & Midi
  | { "kind": "pitchBend" } & Midi
  | { "kind": "hid" } & HidField;

/**
 * What the DJ is told of a controller plugged in while Prisme runs.
 */
export type PluggedIn = { "kind": "opened"; device: string; model: string } | {
  "kind": "offered";
  device: string;
  model: string;
} | { "kind": "waiting"; device: string; model: string };

/**
 * What the settings show of the preferences, and what they offer.
 */
export type Preferences = {
  dj: DjPreferences;
  library: LibraryPreferences;
  ui: UiPreferences;
  updates: UpdatePreferences;
  /**
   * The keyboard's shortcuts in use: the keys the DJ gave, the defaults of the other actions.
   */
  shortcuts: Array<Shortcut>;
  /**
   * The actions whose keys the DJ changed.
   */
  changed: Array<string>;
  choices: SettingChoices;
};

/**
 * What the headphones preview: the track the engine was given, and the file being opened
 * for the preview asked last, if any.
 */
export type PreviewState = {
  track: PreviewTrack | null;
  opening: string | null;
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
 * A track previewed in the headphones.
 */
export type PreviewTrack = {
  /**
   * What the status tells it by.
   */
  id: number;
  path: string;
  title: string;
  artist: string | null;
  /**
   * Estimated until the track is all read.
   */
  durationS: number;
  /**
   * The waveform can be fetched (`preview_waveform`); otherwise a `preview-update` says
   * when it can.
   */
  waveformReady: boolean;
};

/**
 * How many models come from where, their variants aside.
 */
export type ProfileCounts = { all: number; updated: number; own: number };

/**
 * Where a profile comes from.
 */
export type ProfileSource = "shipped" | "updated" | "own";

/**
 * What a knob reads under it: its setting as the DJ reads it, which the screen only words.
 */
export type Readout = { "db": number | null } | { "percent": number } | {
  "balance": number;
} | { "filter": number };

/**
 * A source the Rec menu offers.
 */
export type RecordOption = {
  source: RecordSource;
  /**
   * A turntable comes into these inputs: its timecode would be recorded.
   */
  turntable: boolean;
};

/**
 * What a recording takes.
 */
export type RecordSource = { "kind": "master" } | {
  "kind": "input";
  first: number;
};

/**
 * A recording ended: where it went, and what could not be written.
 */
export type Recording = {
  /**
   * Its first file.
   */
  path: string;
  /**
   * The name of its first file, and the folder it is in.
   */
  fileName: string;
  folder: string;
  /**
   * The files it was written in: the first, then `… 2.wav` past 4 GB or an audio restart.
   */
  parts: number;
  seconds: number;
  /**
   * Tracks in its tracklist.
   */
  tracks: number;
  /**
   * The name of the tracklist written beside it, if it could be.
   */
  tracklist: string | null;
  /**
   * Why the audio was not written whole, if it was not.
   */
  failure: RecordingFailure | null;
};

/**
 * Why a recording was not written whole.
 */
export type RecordingFailure = "diskFull" | "notWritten" | "stopped";

export type RecordingStatus = {
  seconds: number;
  /**
   * Audio the writer could not take in time (lost).
   */
  droppedSeconds: number;
};

/**
 * Where the mixer is heard on the running device, and the live inputs it takes.
 */
export type RoutingState = {
  /**
   * The mode the user chose.
   */
  mode: MixMode;
  /**
   * The software mixer is in use (or would be, the audio off).
   */
  internal: boolean;
  /**
   * An external mixer chosen, the device too small for it: the outputs it needs.
   */
  fallback: number | null;
  /**
   * A controller plugged in with no mixer of its own: Prisme mixes for it (unless the DJ
   * chose otherwise, `all_in_one_mixes`).
   */
  allInOne: string | null;
  allInOneMixes: boolean;
  /**
   * The first output each deck in use plays on to an external mixer (0-based).
   */
  deckOutputs: Array<number>;
  headphones: HeadphonesOutput;
  /**
   * The first output of the headphones in use, if they are heard: with an external mixer,
   * a pair no deck uses, for the preview alone.
   */
  headphonesInUse: number | null;
  /**
   * The pairs the headphones can take, with the one chosen (first outputs).
   */
  headphonesChoices: Array<number>;
  /**
   * Where the headphones are heard on the running device.
   */
  headphonesNote: OutputNote | null;
  /**
   * The first output of the booth chosen, and the one in use if it is heard.
   */
  booth: number | null;
  boothInUse: number | null;
  boothChoices: Array<BoothChoice>;
  /**
   * Where the booth chosen is heard, or why not, on the software mixer of the running device.
   */
  boothNote: OutputNote | null;
  mic: LiveState;
  aux: LiveState;
  /**
   * What can be recorded on the running device.
   */
  recordOptions: Array<RecordOption>;
};

export type RowPick = "only" | "toggle" | "range" | "context";

/**
 * How far a row's track is separated, as its column shows it.
 */
export type RowStems =
  | { "state": "separating"; fraction: number }
  | { "state": "queued" }
  | { "state": "addable"; separation: Separation }
  | { "state": "separated"; separation: Separation };

/**
 * The speed the record turns at when the track plays at its own speed.
 */
export type Rpm = "33" | "45";

/**
 * Bounds are inclusive; a missing bound is open.
 */
export type Rule =
  | { "kind": "text"; field: TextField; contains: string }
  | { "kind": "bpm"; min: number | null; max: number | null }
  | { "kind": "year"; min: number | null; max: number | null }
  | { "kind": "key"; camelot: Array<string> };

/**
 * A screen plugged in.
 */
export type Screen = {
  /**
   * What the system knows the screen itself by (see `screen_id`), by which it is chosen.
   */
  id: string;
  /**
   * The system's name of where it is plugged (`\\.\DISPLAY2`).
   */
  name: string;
  /**
   * Its place and size on the desktop (physical px).
   */
  x: number;
  y: number;
  width: number;
  height: number;
  /**
   * The main window is on it.
   */
  main: boolean;
};

export type SearchScope = "list" | "library";

/**
 * What became of tracks to separate.
 */
export type Separated = "queued" | "needsModels";

/**
 * How far a track is separated, the further the greater.
 */
export type Separation = "none" | "vocals" | "full";

/**
 * One setting changed in the settings (the quantize and the tempo range are the actions';
 * the music folders have their own commands).
 */
export type Setting =
  | { "setting": "vinylSync"; "value": VinylSyncMode }
  | { "setting": "autoGain"; "value": boolean }
  | { "setting": "deckCount"; "value": number }
  | { "setting": "knobMagnet"; "value": boolean }
  | { "setting": "lockControls"; "value": boolean }
  | { "setting": "cueOnLoad"; "value": boolean }
  | { "setting": "protectPlaying"; "value": boolean }
  | { "setting": "column"; "value": { column: string; shown: boolean } }
  | { "setting": "writeTags"; "value": boolean }
  | { "setting": "replaceTags"; "value": boolean }
  | { "setting": "pauseWhilePlaying"; "value": boolean }
  | { "setting": "stemsOnLoad"; "value": boolean }
  | { "setting": "stemsGpu"; "value": boolean }
  | { "setting": "stemsFast"; "value": boolean }
  | { "setting": "language"; "value": string | null }
  | { "setting": "phoneTouch"; "value": boolean | null }
  | { "setting": "startFullscreen"; "value": boolean }
  | { "setting": "decksLayout"; "value": DecksLayout }
  | { "setting": "allInOneMixes"; "value": boolean }
  | { "setting": "automaticUpdates"; "value": boolean };

/**
 * The values the settings offer where there are a few.
 */
export type SettingChoices = {
  tempoRanges: [number, number, number];
  deckCounts: [number, number];
  talkoverDepths: [number, number, number, number];
  languages: [string, string];
  /**
   * The lengths the Auto DJ's mixes are offered at: in beats, and in seconds.
   */
  fadeBeats: [number, number, number, number];
  fadeSeconds: [number, number, number, number];
};

/**
 * A list of the side panel a controller points at.
 */
export type Shelf = { "kind": "prepare" } | { "kind": "crate"; id: number };

/**
 * The layer of a control: the first, the second while any shift is held (`true`), or a named
 * shift's while that one is held (`"left"`).
 */
export type Shift = boolean | string;

/**
 * A key of the keyboard bound to an action.
 */
export type Shortcut = {
  /**
   * It acts while typing and over the settings and the palette.
   */
  everywhere: boolean;
  /**
   * Its action is of a deck in use, or of none.
   */
  inUse: boolean;
  control: string;
  code: string;
  ctrl?: boolean;
  shift?: boolean;
};

/**
 * The timecode as a deck hears it: none, a signal that comes and goes, a good one.
 */
export type SignalState = "none" | "bounce" | "ok";

export type SmartRules = {
  /**
   * Every rule must match; otherwise one is enough.
   */
  matchAll: boolean;
  rules: Array<Rule>;
};

export type SortColumn =
  | "order"
  | "title"
  | "artist"
  | "album"
  | "genre"
  | "year"
  | "bpm"
  | "key"
  | "rating"
  | "plays"
  | "mix";

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
   * It can be turned: its part separated where the deck plays.
   */
  ready: boolean;
};

/**
 * A part's level: where its knob stands (0 to 1), kept while it is muted.
 */
export type StemLevel = { level: number; on: boolean };

/**
 * How a deck plays each part of the track.
 */
export type StemMix = {
  vocals: StemLevel;
  drums: StemLevel;
  bass: StemLevel;
  /**
   * What no stem holds: the instrumental of a track separated into its vocals alone.
   */
  other: StemLevel;
};

export type StemPart = "vocals" | "instrumental" | "drums" | "bass" | "melody";

/**
 * The separation queue.
 */
export type StemsProgress = {
  running: boolean;
  /**
   * Tracks left, the one being separated included.
   */
  queued: number;
  done: number;
  failed: number;
  /**
   * The track being separated, and how far (0 to 1).
   */
  current: string | null;
  fraction: number;
  /**
   * The GPU (and API) or "CPU" doing the work.
   */
  device: string | null;
};

/**
 * The separation as the interface shows it (`stems_service::Shown`).
 */
export type StemsState = {
  progress: StemsProgress;
  /**
   * The tracks the queue held: those done, failed, and left.
   */
  total: number;
  /**
   * The queue's share done, the track being separated as far as it is, from 0 to 1.
   */
  overall: number;
  /**
   * How far the models chosen and installed separate a track.
   */
  reach: Separation;
  /**
   * A graphics chip worth separating on is there; `None` until it was looked for.
   */
  gpu: boolean | null;
  /**
   * The separation runs on it: it is there and the DJ lets it.
   */
  onGpu: boolean;
  /**
   * Tracks are separated as they are loaded or prepared.
   */
  onLoad: boolean;
  /**
   * How far the downloads under way are, by their bytes, from 0 to 1.
   */
  downloading: number | null;
  /**
   * The models the DJ chose, as the settings list them.
   */
  models: Array<ModelRow>;
  /**
   * What the large models, and the small one, have left to download.
   */
  quality: ModelChoice;
  fast: ModelChoice;
  /**
   * The small models suit this device: a phone, or no graphics chip worth it.
   */
  fastSuits: boolean;
  /**
   * The choice the models are offered with.
   */
  suggestFast: boolean;
};

/**
 * Where a folder to add to the music folders is looked for first, named by its own folder
 * (`3A2F-1C0B`, `C:`).
 */
export type StoragePlace = {
  kind: StoragePlaceKind;
  name: string;
  path: string;
};

/**
 * What holds a [`StoragePlaceDto`].
 */
export type StoragePlaceKind = "phone" | "removable" | "home" | "drive";

/**
 * The knobs of a deck's channel: trim, EQ (low, mid, high) and filter.
 */
export type StripKnobs = {
  trim: KnobState;
  eq: [KnobState, KnobState, KnobState];
  filter: KnobState;
};

/**
 * What a deck's SYNC key does.
 */
export type SyncKey = "lock" | "once" | "noGrid" | "absolute";

/**
 * A deck's sync.
 */
export type SyncState = "off" | "matched" | "locked";

export type SyncStatus = {
  state: SyncState;
  leader: boolean;
  /**
   * What its SYNC key does.
   */
  key: SyncKey;
};

/**
 * A key another action has, which a change of keys would take from it.
 */
export type TakenKey = {
  /**
   * The action that has it, as the interface names it.
   */
  action: Target | null;
  control: string;
  code: string;
  ctrl?: boolean;
  shift?: boolean;
};

/**
 * An action a key or a controller can drive, as the interface lists it: for the keys, the
 * controllers' mappings and the command palette.
 */
export type Target = {
  id: string;
  kind: ControlKind;
  /**
   * The deck it acts on: a deck's, or a load's.
   */
  deck: number | null;
  group: Label;
  label: Label;
  /**
   * Of a deck in use, or of none.
   */
  inUse: boolean;
};

export type Tempo = {
  /**
   * Its tempo: the mean of one that changes.
   */
  bpm: number;
  firstBeatS: number;
  /**
   * The beats of a bar, the first on `first_beat_s`.
   */
  beatsPerBar: number;
  /**
   * Where a tempo that changes is pinned, steady between two: none for a steady one.
   */
  anchors: Array<GridAnchor>;
  /**
   * The tempo past the last anchor.
   */
  bpmAfter: number;
};

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

export type TextField =
  | "title"
  | "artist"
  | "album"
  | "genre"
  | "comment"
  | "fileName";

export type TimecodeFormat = {
  id: string;
  maker: string;
  record: string;
  side: string | null;
  leadInS: number;
  /**
   * Read on the record itself, not only on signals made from its description.
   */
  verified: boolean;
};

/**
 * The timecode records of a maker.
 */
export type TimecodeMaker = { name: string; records: Array<TimecodeRecord> };

/**
 * A timecode record (or CD), a format for each side when its sides differ.
 */
export type TimecodeRecord = { name: string; sides: Array<TimecodeFormat> };

export type TrackInfo = {
  deck: number;
  /**
   * Tells this load from another of the same file.
   */
  loadId: number;
  path: string;
  title: string;
  artist: string | null;
  durationS: number;
  sampleRate: number;
  tempo: Tempo | null;
  key: MusicalKey | null;
  gridEdited: boolean;
  keyEdited: boolean;
  /**
   * Its bars are to be checked by ear (`prisme_analysis::Tempo::bars_sure`).
   */
  barsToCheck: boolean;
  cues: Array<Cue>;
  /**
   * Semitones the deck transposes the track by.
   */
  keyShift: number;
  stemMix: StemMix;
  separation: Separation;
  /**
   * Separated in parts only: being separated, or a separation stopped short.
   */
  partial: boolean;
};

/**
 * The list browsed, as the interface names it.
 */
export type TrackSource =
  | { "kind": "folder"; path: string }
  | { "kind": "prepare" }
  | { "kind": "duplicates" }
  | { "kind": "elsewhere" }
  | { "kind": "crate"; id: number }
  | { "kind": "history"; id: number };

/**
 * An action the interface performs that a controller drove: a key's value, or an encoder's
 * whole steps.
 */
export type UiAction = { target: string; value: number };

/**
 * How the interface shows itself, chosen in it.
 */
export type UiPreferences = {
  /**
   * One of [`UiPreferences::LANGUAGES`], else the system's.
   */
  language: string | null;
  /**
   * On a phone, the decks' touch controls shown or not, else as what is plugged in says.
   */
  phoneTouch: boolean | null;
  /**
   * The main window full screen at the start, not only filling its screen.
   */
  startFullscreen: boolean;
  /**
   * The screen the decks are on in a window of their own (its id, see `list_screens`), else in the main
   * window.
   */
  decksScreen: string | null;
  /**
   * How the decks are laid out in their own window.
   */
  decksLayout: DecksLayout;
};

/**
 * How a computer brings Prisme's new versions.
 */
export type UpdatePreferences = {
  /**
   * Downloaded and put in place by themselves, for the next start; else when the DJ asks.
   */
  automatic: boolean;
};

/**
 * Where the new versions stand, as the page shows it.
 */
export type UpdateState =
  | { "stage": "unknown" }
  | { "stage": "looking" }
  | { "stage": "latest" }
  | { "stage": "unreachable" }
  | { "stage": "available"; version: string; installsItself: boolean }
  | { "stage": "downloading"; version: string; progress: number }
  | { "stage": "ready"; version: string }
  | { "stage": "failed"; version: string; error: string };

/**
 * A setting of a deck's vinyl control, changed alone: how the record drives the deck, how fast
 * it turns, where on it the track starts (`None`: where the record's own lead-in ends). It is
 * put on and off by an action of the deck (`deckN.vinyl`).
 */
export type VinylChange = { "setting": "mode"; "value": VinylMode } | {
  "setting": "rpm";
  "value": Rpm;
} | { "setting": "leadIn"; "value": number | null };

export type VinylMode = "absolute" | "relative";

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

/**
 * What SYNC does on a record in relative mode.
 */
export type VinylSyncMode = "once" | "locked";

/**
 * A mode of a controller: `mode` of its group `group`.
 */
export type When = { group: string; mode: string };
