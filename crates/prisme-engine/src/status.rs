//! Snapshots of the engine state, published by the audio thread for the UI.

use crate::command::VinylMode;
use crate::looping::LoopRegion;
use prisme_timecode::SignalState;

/// Maximum number of decks.
pub const MAX_DECKS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VinylStatus {
    pub enabled: bool,
    pub mode: VinylMode,
    /// The needle reached the end of the record: the deck plays on by itself.
    pub run_out: bool,
    /// The record has read backwards at play speed for a while: its left and right
    /// channels are probably crossed.
    pub crossed: bool,
    pub signal: SignalState,
    /// Timecode carrier level (full scale = 1.0).
    pub level: f32,
    /// Speed read from the record, 1.0 at its [`Rpm`](crate::Rpm).
    pub record_speed: f64,
    /// Absolute position read from the record, in seconds at its [`Rpm`](crate::Rpm),
    /// once locked.
    pub record_position_s: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SyncStatus {
    pub locked: bool,
    pub leader: bool,
    /// Relative vinyl: factor applied to the platter speed by a one-shot sync.
    pub rate_offset: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LoopStatus {
    /// The stored loop, active or not.
    pub region: Option<LoopRegion>,
    /// How many beats it lasts, on the deck's grid.
    pub beats: Option<f64>,
    pub active: bool,
    /// Start marked by LOOP IN, waiting for LOOP OUT.
    pub loop_in_s: Option<f64>,
    /// A roll or a slice is held: its loop is the one shown, the stored one back once it is
    /// let go.
    pub rolling: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeckStatus {
    pub loaded: bool,
    /// Internal play state (vinyl control off).
    pub playing: bool,
    pub position_s: f64,
    pub duration_s: f64,
    /// Effective playback speed (1.0 = original tempo, negative = backwards).
    pub speed: f64,
    /// Current tempo as a playback rate, gestures excluded (1.0 = the track's own).
    pub rate: f64,
    /// Current tempo, gestures excluded (needs a beat grid).
    pub bpm: Option<f64>,
    /// Position within the current beat, `0..1`.
    pub beat_phase: Option<f64>,
    pub sync: SyncStatus,
    pub looping: LoopStatus,
    /// Loudest sample since the previous snapshot.
    pub peak: f32,
    /// Sampler slots playing (bit per slot).
    pub samples_playing: u16,
    /// Internal mixer: heard in the headphones.
    pub cue: bool,
    pub slip_mode: bool,
    /// The reverse is on: the track plays backwards.
    pub reverse: bool,
    /// Where the track would be without the loop, roll, censor or reverse in progress.
    pub slip_position_s: Option<f64>,
    /// The parts whose change waits for the next bar, a bit each: vocals, drums, bass, other.
    pub stems_waiting: u8,
    /// The parts separated where the deck plays, the same bits.
    pub stems_here: u8,
    pub vinyl: VinylStatus,
}

impl Default for DeckStatus {
    fn default() -> Self {
        Self {
            loaded: false,
            playing: false,
            position_s: 0.0,
            duration_s: 0.0,
            speed: 0.0,
            rate: 1.0,
            bpm: None,
            beat_phase: None,
            sync: SyncStatus { rate_offset: 1.0, ..Default::default() },
            looping: LoopStatus::default(),
            peak: 0.0,
            samples_playing: 0,
            cue: false,
            slip_mode: false,
            reverse: false,
            slip_position_s: None,
            stems_waiting: 0,
            stems_here: 0,
            vinyl: VinylStatus {
                enabled: false,
                mode: VinylMode::Absolute,
                run_out: false,
                crossed: false,
                signal: SignalState::None,
                level: 0.0,
                record_speed: 0.0,
                record_position_s: None,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EngineStatus {
    pub deck_count: usize,
    pub decks: [DeckStatus; MAX_DECKS],
    /// Peak level of the main output since the previous snapshot.
    pub main_peak: f32,
    /// Peak levels of the software mixer's master, left and right, since the previous
    /// snapshot (silence with an external mixer).
    pub master_peaks: [f32; 2],
    /// Peak level of the microphone and of the aux line at their levels, on or not.
    pub mic_peak: f32,
    pub aux_peak: f32,
    pub recording: Option<RecordingStatus>,
    /// The track heard in the headphones before it goes on a deck, if any.
    pub preview: Option<PreviewStatus>,
    /// Commands the engine had applied (see [`EngineController::sent`]); none in the status it
    /// has before it first runs.
    ///
    /// [`EngineController::sent`]: crate::EngineController::sent
    pub commands: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewStatus {
    /// As given when it started.
    pub id: u64,
    pub position_s: f64,
    /// Estimated until the track is all decoded.
    pub duration_s: f64,
    /// Its audio is not decoded that far yet: it waits, silent.
    pub waiting: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecordingStatus {
    pub frames: u64,
    /// Frames the writer could not take in time (lost).
    pub dropped: u64,
}
