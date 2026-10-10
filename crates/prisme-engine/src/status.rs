//! Snapshots of the engine state, published by the audio thread for the UI.

use crate::command::VinylMode;
use prisme_timecode::SignalState;

/// Maximum number of decks.
pub const MAX_DECKS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VinylStatus {
    pub enabled: bool,
    pub mode: VinylMode,
    pub signal: SignalState,
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
            vinyl: VinylStatus {
                enabled: false,
                mode: VinylMode::Absolute,
                signal: SignalState::None,
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
}
