//! Messages between the control side (UI, controllers) and the audio thread.

use crate::grid::BeatGrid;
use crate::live::{InputChannels, LiveSource};
use crate::looping::LoopRegion;
use crate::mixer::{CrossfaderCurve, OutputRouting};
use crate::stems::{Parts, StemMix, StemTracks};
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

    /// The music `record_s` seconds of grooves at 33⅓ rpm hold at that speed (s): less at 45 rpm,
    /// the grooves going by faster.
    pub fn music_s(self, record_s: f64) -> f64 {
        record_s / self.record_speed()
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
    /// How the deck plays each part of the track (all at full level again at each load); the
    /// parts it changes from the next bar if `on_bar` and the deck moves forward with a beat
    /// grid, else at once.
    SetStemMix {
        deck: DeckId,
        mix: StemMix,
        on_bar: bool,
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
    /// Internal play: a playing deck slows to a stop in about a second, as a turntable
    /// switched off; a stopped one gets up to speed in half a second (a soft start).
    Brake {
        deck: DeckId,
    },
    /// Internal play: the record spun back fast while `held`; let go, it plays on from there.
    SpinBack {
        deck: DeckId,
        held: bool,
    },
    /// The deck's channel silenced in the master, after its fader (the headphones still hear
    /// it), or on its own output to an external mixer; or heard again.
    SetMute {
        deck: DeckId,
        muted: bool,
    },
    /// At the end, the track goes on from its start, seamlessly: it plays on.
    SetRepeat {
        deck: DeckId,
        enabled: bool,
    },
    /// Moves the playhead there, as is (needle search).
    Seek {
        deck: DeckId,
        seconds: f64,
    },
    /// Moves the playhead by `seconds` (back below zero) within the track, as is (a jog
    /// searching).
    SearchBy {
        deck: DeckId,
        seconds: f64,
    },
    /// While `speed` is not 0, the track runs through at `speed` seconds a second beside its
    /// play (back below zero), by jumps a few times a second; 0 lets the search go.
    Search {
        deck: DeckId,
        speed: f64,
    },
    /// Puts `deck` where `from` is in the same track, in its loop, playing if it plays: an
    /// instant double.
    TakePlace {
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
    /// `Some(region)`: plays that slice of the track in a loop while held, the track going
    /// on underneath (in phase with quantize); `None` releases it.
    Slice {
        deck: DeckId,
        region: Option<LoopRegion>,
    },
    ExitLoop {
        deck: DeckId,
    },
    /// While `Some(beats)`, the deck is cut every so many beats on its grid (open the first
    /// half of each), as a hand flicking its fader; `None` lets it through.
    FaderCut {
        deck: DeckId,
        beats: Option<f64>,
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
    /// The track plays backwards at the deck's rate, or forwards again; with slip, it goes on
    /// underneath and comes back where it would be.
    SetReverse {
        deck: DeckId,
        enabled: bool,
    },
    /// Backwards while `held` whatever the reverse, as it was afterwards; with slip, as
    /// [`Command::SetReverse`].
    HoldReverse {
        deck: DeckId,
        held: bool,
    },
    /// Off, a record that drove the deck leaves it playing by itself as it played it.
    SetVinylEnabled {
        deck: DeckId,
        enabled: bool,
    },
    SetVinylMode {
        deck: DeckId,
        mode: VinylMode,
    },
    /// Position of the start of the track on the record, in seconds at its [`Rpm`]
    /// (lead-in); `None`, the record's own.
    SetLeadIn {
        deck: DeckId,
        seconds: Option<f64>,
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
    /// The deck that led as the audio stopped to start again, chosen or not: it leads again,
    /// no other deck taking the lead while it gets back up to speed.
    ResumeSyncLeader {
        deck: DeckId,
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
    /// Headphones: the cue on the left and the master on the right (mono each), or blended.
    SetSplitCue {
        enabled: bool,
    },
    /// The crossfader's sides swapped: towards the left, the decks on the right come in.
    SetCrossfaderReverse {
        enabled: bool,
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
    /// The effect in a slot of the deck's chain (`slot` < [`FX_SLOTS`](crate::FX_SLOTS)).
    SetFx {
        deck: DeckId,
        slot: usize,
        kind: EffectKind,
    },
    SetFxEnabled {
        deck: DeckId,
        slot: usize,
        enabled: bool,
    },
    /// Level of the slot's effect, 0..1.
    SetFxMix {
        deck: DeckId,
        slot: usize,
        mix: f32,
    },
    /// The slot's main knob (feedback, depth...), 0..1.
    SetFxAmount {
        deck: DeckId,
        slot: usize,
        amount: f32,
    },
    /// The slot's second knob (tone, resonance...), 0..1.
    SetFxParam2 {
        deck: DeckId,
        slot: usize,
        param2: f32,
    },
    /// The slot's period in beats.
    SetFxBeats {
        deck: DeckId,
        slot: usize,
        beats: f32,
    },
    /// The deck's whole chain on or off, each slot keeping its own switch.
    SetFxChainEnabled {
        deck: DeckId,
        enabled: bool,
    },
    /// The parts of the track the deck's effects are on, where they are separated; none: the
    /// whole deck, after its channel strip.
    SetFxParts {
        deck: DeckId,
        parts: Parts,
    },
    /// Puts a sound in a sampler slot of the deck (`None` empties it).
    SetSample {
        deck: DeckId,
        slot: usize,
        sample: Option<Arc<TrackAudio>>,
    },
    /// Plays a sampler slot from its start (on the next beat with quantize on), at `gain`
    /// times the sampler's level: how hard its pad was hit, 1 for a key.
    TriggerSample {
        deck: DeckId,
        slot: usize,
        gain: f32,
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
    /// Plays `track` in the headphones alone from `start_s`, in place of the one previewed;
    /// `id` tells it in the status.
    Preview {
        id: u64,
        track: Arc<TrackAudio>,
        start_s: f64,
    },
    SeekPreview {
        seconds: f64,
    },
    /// Moves the preview by `seconds`, forwards or back.
    SkipPreview {
        seconds: f64,
    },
    StopPreview,
    /// Software mixer: -1 = the master on the left only, 0 = both sides, 1 = the right only.
    SetBalance {
        balance: f32,
    },
    /// Software mixer: the level of the booth monitors (linear).
    SetBoothGain {
        gain: f32,
    },
    /// Software mixer: the input channels a live source comes in on (`None`: not plugged).
    SetLiveInput {
        source: LiveSource,
        channels: Option<InputChannels>,
    },
    /// The level of a live source (linear).
    SetLiveLevel {
        source: LiveSource,
        gain: f32,
    },
    /// A live source heard in the master, or not.
    SetLiveOn {
        source: LiveSource,
        on: bool,
    },
    /// A live source heard in the headphones, on or not.
    SetLiveCue {
        source: LiveSource,
        cue: bool,
    },
    /// While the microphone is on, the music ducks by `depth_db`, if `enabled`.
    SetTalkover {
        enabled: bool,
        depth_db: f32,
    },
    /// The deck plays its input as it comes (a record, a CD player) instead of its track,
    /// through its channel.
    SetPassthrough {
        deck: DeckId,
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
    Grid(BeatGrid),
    Decoder(Box<TimecodeDecoder>),
    Recording(rtrb::Producer<f32>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lead_in_holds_less_music_at_45_rpm() {
        assert_eq!(Rpm::Rpm33.music_s(28.0), 28.0);
        assert!((Rpm::Rpm45.music_s(28.0) - 20.74).abs() < 0.01);
        assert_eq!(Rpm::Rpm45.music_s(0.0), 0.0);
    }
}
