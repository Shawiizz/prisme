//! Messages between the control side (UI, controllers) and the audio thread.

use crate::grid::BeatGrid;
use crate::looping::LoopRegion;
use crate::mixer::{CrossfaderCurve, OutputRouting};
use crate::stems::{StemMix, StemTracks};
use prisme_dsp::{Band, EffectKind};
use prisme_media::TrackAudio;
use prisme_timecode::TimecodeDecoder;
use std::sync::Arc;

/// Index of a deck (0-based).
pub type DeckId = usize;

/// How a deck's speed and position are driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VinylMode {
    /// The record drives the speed and the absolute position (needle drops move the track).
    #[default]
    Absolute,
    /// The record drives the speed only.
    Relative,
}

/// The speed the record turns at when the track plays at its own speed: at 45 rpm the
/// music takes 35 % more groove.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rpm {
    #[default]
    Rpm33,
    Rpm45,
}

impl Rpm {
    /// The record's speed, as read from the timecode, that plays the track at its own speed.
    pub fn record_speed(self) -> f64 {
        match self {
            Rpm::Rpm33 => 1.0,
            Rpm::Rpm45 => 1.35,
        }
    }

    /// How long a turn lasts at that speed (s): 1.8 at 33⅓ rpm.
    pub fn turn_s(self) -> f64 {
        1.8 / self.record_speed()
    }
}

/// Which side of the crossfader a deck is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrossfaderSide {
    Left,
    #[default]
    Center,
    Right,
}

/// Commands sent to the engine. Building them may allocate; executing them never does.
pub enum Command {
    LoadTrack {
        deck: DeckId,
        track: Arc<TrackAudio>,
    },
    Eject {
        deck: DeckId,
    },
    /// The separated stems of the loaded track: same rate and length (`None`: not separated).
    SetStems {
        deck: DeckId,
        stems: StemTracks,
    },
    /// Which stems the deck plays (all again when a track is loaded).
    /// How the deck plays each part of the track; the parts it changes from the next bar if
    /// `on_bar` and the deck moves forward with a beat grid, else at once.
    SetStemMix {
        on_bar: bool,
        deck: DeckId,
        mix: StemMix,
    },
    /// Internal play (used when vinyl control is off).
    SetPlaying {
        deck: DeckId,
        playing: bool,
    },
    /// Tempo ratio for internal play (1.0 = original tempo).
    SetPitch {
        deck: DeckId,
        ratio: f64,
    },
    /// Internal play: temporary speed change while held (0.04 = +4 %, 0 = released).
    SetPitchBend {
        deck: DeckId,
        amount: f64,
    },
    /// Moves the playhead there, as is (needle search).
    Seek {
        deck: DeckId,
        seconds: f64,
    },
    /// Moves the playhead of `deck` where that of `from` is, and plays it if `from` plays: with
    /// the same track loaded, an instant double.
    TakePosition {
        deck: DeckId,
        from: DeckId,
    },
    /// A controller's jog wheel touched or let go, at `at_s` on the controller's clock (s).
    JogTouch {
        deck: DeckId,
        touched: bool,
        at_s: f64,
    },
    /// A controller's jog wheel turned by `turns` (positive forwards): touched it scratches,
    /// by its edge it nudges a playing track, a stopped one moves.
    Jog {
        deck: DeckId,
        turns: f64,
        at_s: f64,
    },
    /// The track dragged by hand to `position_s` at `at_s` on the pointer's clock (the
    /// scrolling waveform moved like a record), or let go (`None`).
    Hand {
        deck: DeckId,
        position_s: Option<f64>,
        at_s: f64,
    },
    /// Moves the playhead there, keeping the beat phase if quantize is on (cues).
    Jump {
        deck: DeckId,
        seconds: f64,
    },
    BeatJump {
        deck: DeckId,
        beats: f64,
    },
    SetQuantize {
        enabled: bool,
    },
    SetLoop {
        deck: DeckId,
        region: Option<LoopRegion>,
        active: bool,
    },
    BeatLoop {
        deck: DeckId,
        beats: f64,
    },
    /// Marks the start of the next loop (on the nearest beat with quantize).
    LoopIn {
        deck: DeckId,
    },
    /// Ends the loop at the playhead and plays it.
    LoopOut {
        deck: DeckId,
    },
    /// `Some(beats)` starts a roll, `None` releases it.
    LoopRoll {
        deck: DeckId,
        beats: Option<f64>,
    },
    ExitLoop {
        deck: DeckId,
    },
    Reloop {
        deck: DeckId,
    },
    ScaleLoop {
        deck: DeckId,
        factor: f64,
    },
    SetSlip {
        deck: DeckId,
        enabled: bool,
    },
    Censor {
        deck: DeckId,
        active: bool,
    },
    SetVinylEnabled {
        deck: DeckId,
        enabled: bool,
    },
    SetVinylMode {
        deck: DeckId,
        mode: VinylMode,
    },
    /// Position of the start of the track on the record, in seconds at its [`Rpm`]
    /// (lead-in).
    SetLeadIn {
        deck: DeckId,
        seconds: f64,
    },
    SetRpm {
        deck: DeckId,
        rpm: Rpm,
    },
    /// Replaces a deck's timecode decoder (another format, swapped channels...). Build it
    /// on the control side: it allocates.
    ReplaceDecoder {
        deck: DeckId,
        decoder: Box<TimecodeDecoder>,
    },
    SetBeatGrid {
        deck: DeckId,
        grid: Option<BeatGrid>,
    },
    /// Locks the deck to the leader's tempo (`false` also clears a one-shot match).
    SetSyncLocked {
        deck: DeckId,
        locked: bool,
    },
    /// Relative vinyl: matches the leader's tempo and phase once. Internal play: locks.
    SyncOnce {
        deck: DeckId,
    },
    /// `None`: the leader is chosen automatically among the playing decks.
    SetSyncLeader {
        deck: Option<DeckId>,
    },
    SetVolume {
        deck: DeckId,
        gain: f32,
    },
    SetCrossfaderSide {
        deck: DeckId,
        side: CrossfaderSide,
    },
    /// -1.0 = full left, 1.0 = full right.
    SetCrossfader {
        position: f32,
    },
    SetMasterGain {
        gain: f32,
    },
    SetRouting {
        routing: OutputRouting,
    },
    /// Internal mixer: sends the deck to the headphones.
    SetDeckCue {
        deck: DeckId,
        enabled: bool,
    },
    /// Headphones: 0 = cued decks only, 1 = master only.
    SetCueMix {
        mix: f32,
    },
    SetHeadphoneGain {
        gain: f32,
    },
    /// The level of the track on `deck` (linear): its auto gain, in either mixer mode.
    SetLevel {
        deck: DeckId,
        gain: f32,
    },
    /// Internal mixer: gain before the EQ (linear).
    SetTrim {
        deck: DeckId,
        gain: f32,
    },
    /// Internal mixer: EQ band gain (linear, 1 = flat).
    SetEq {
        deck: DeckId,
        band: Band,
        gain: f32,
    },
    SetEqKill {
        deck: DeckId,
        band: Band,
        kill: bool,
    },
    /// Internal mixer: -1 = low-pass closed, 0 = off, 1 = high-pass open.
    SetFilter {
        deck: DeckId,
        position: f32,
    },
    /// The deck's beat effect.
    SetFx {
        deck: DeckId,
        kind: EffectKind,
    },
    SetFxEnabled {
        deck: DeckId,
        enabled: bool,
    },
    /// Level of the effect, 0..1.
    SetFxMix {
        deck: DeckId,
        mix: f32,
    },
    /// The effect's main knob (feedback, depth...), 0..1.
    SetFxAmount {
        deck: DeckId,
        amount: f32,
    },
    /// The effect's period in beats.
    SetFxBeats {
        deck: DeckId,
        beats: f32,
    },
    /// Puts a sound in a sampler slot of the deck (`None` empties it).
    SetSample {
        deck: DeckId,
        slot: usize,
        sample: Option<Arc<TrackAudio>>,
    },
    /// Plays a sampler slot from its start (on the next beat with quantize on).
    TriggerSample {
        deck: DeckId,
        slot: usize,
    },
    StopSample {
        deck: DeckId,
        slot: usize,
    },
    SetSamplerGain {
        deck: DeckId,
        gain: f32,
    },
    /// Starts copying `source` into `sink` (a running recording is replaced).
    StartRecording {
        source: RecordSource,
        sink: rtrb::Producer<f32>,
    },
    StopRecording,
    /// The pitch stays when the speed changes.
    SetKeyLock {
        deck: DeckId,
        enabled: bool,
    },
    /// Transposes the track by whole semitones.
    SetKeyShift {
        deck: DeckId,
        semitones: i8,
    },
    SetCrossfaderCurve {
        curve: CrossfaderCurve,
    },
    SetLimiter {
        enabled: bool,
    },
}

/// What a recording takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordSource {
    /// The software mixer's master.
    Master,
    /// An input pair (its first channel), e.g. a hardware mixer's record output.
    Input(usize),
}

/// Objects the audio thread hands back so that they are freed on the control side.
pub enum Garbage {
    Track(Arc<TrackAudio>),
    Decoder(Box<TimecodeDecoder>),
    Recording(rtrb::Producer<f32>),
}
