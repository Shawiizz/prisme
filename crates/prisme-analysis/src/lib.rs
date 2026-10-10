//! Track analysis, run off the audio thread when a track is loaded.
//!
//! * [`waveform`]: amplitude and low / mid / high energy per time slice, for drawing
//!   colored waveforms.
//! * [`tempo`]: BPM and beat grid.
//! * [`key`]: musical key, with its Camelot code.
//!
//! Tempo and key work on a mono copy decimated to ~11 kHz ([`dsp`]), and the tempo on
//! onset envelopes ([`onset`]).

pub mod dsp;
pub mod key;
pub mod onset;
pub mod tempo;
pub mod waveform;

pub use key::{Key, KeyConfig, KeyEstimate, Mode, Profiles};
pub use tempo::{Tempo, DEFAULT_MIN_BPM};
pub use waveform::{compute_waveform, Waveform};

use prisme_media::TrackAudio;

#[derive(Debug, Clone, Copy)]
pub struct AnalysisOptions {
    /// The BPM is reported in `[min_bpm, 2 * min_bpm)`.
    pub min_bpm: f64,
}

impl Default for AnalysisOptions {
    fn default() -> Self {
        Self { min_bpm: DEFAULT_MIN_BPM }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackAnalysis {
    pub tempo: Option<Tempo>,
    pub key: Option<KeyEstimate>,
}

/// Tempo and key of a track.
pub fn analyze(track: &TrackAudio, options: &AnalysisOptions) -> TrackAnalysis {
    let signal = dsp::analysis_signal(track);
    let envelopes = onset::onset_envelopes(&signal);
    TrackAnalysis {
        tempo: tempo::estimate_tempo(&envelopes, options.min_bpm),
        key: key::estimate_key(&signal),
    }
}
