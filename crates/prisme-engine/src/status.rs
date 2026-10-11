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
    pub signal: SignalState,
    /// Timecode carrier level (full scale = 1.0).
    pub level: f32,
    /// Speed read from the record.
    pub record_speed: f64,
    /// Absolute position read from the record, in seconds, once locked.
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
    pub active: bool,
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
    /// Current tempo, gestures excluded (needs a beat grid).
    pub bpm: Option<f64>,
    /// Position within the current beat, `0..1`.
    pub beat_phase: Option<f64>,
    pub sync: SyncStatus,
    pub looping: LoopStatus,
    /// Loudest sample since the previous snapshot.
    pub peak: f32,
    /// Sampler slots playing (bit per slot).
    pub samples_playing: u8,
    /// Internal mixer: heard in the headphones.
    pub cue: bool,
    pub slip_mode: bool,
    /// Where the track would be without the loop, roll or censor in progress.
    pub slip_position_s: Option<f64>,
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
            bpm: None,
            beat_phase: None,
            sync: SyncStatus { rate_offset: 1.0, ..Default::default() },
            looping: LoopStatus::default(),
            peak: 0.0,
            samples_playing: 0,
            cue: false,
            slip_mode: false,
            slip_position_s: None,
            vinyl: VinylStatus {
                enabled: false,
                mode: VinylMode::Absolute,
                run_out: false,
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
    pub recording: Option<RecordingStatus>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecordingStatus {
    pub frames: u64,
    /// Frames the writer could not take in time (lost).
    pub dropped: u64,
}
