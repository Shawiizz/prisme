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

use prisme_media::{DecodeStream, MediaError, TrackAudio};
use std::path::Path;

/// Version of the tempo / key algorithms. Stored results from an older version are
/// recomputed: bump it whenever a change can alter them.
pub const ANALYSIS_VERSION: u32 = 1;

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

/// Tempo and key of a decoded track.
pub fn analyze(track: &TrackAudio, options: &AnalysisOptions) -> TrackAnalysis {
    analyze_signal(&dsp::analysis_signal(track), options)
}

/// Tempo and key of a file, decoded on the fly: memory stays at the size of the
/// decimated signal (~13 MB for 5 minutes) instead of the whole track.
pub fn analyze_file(path: &Path, options: &AnalysisOptions) -> Result<FileAnalysis, MediaError> {
    let mut stream = DecodeStream::open(path)?;
    let mut decimator = dsp::Decimator::new(stream.sample_rate());
    let mut chunk = Vec::new();
    while stream.next_chunk(&mut chunk)? {
        decimator.push(&chunk);
        chunk.clear();
    }
    let signal = decimator.finish();
    if signal.samples.is_empty() {
        return Err(MediaError::NoAudio);
    }
    Ok(FileAnalysis { duration_s: signal.duration_s(), analysis: analyze_signal(&signal, options) })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FileAnalysis {
    pub duration_s: f64,
    pub analysis: TrackAnalysis,
}

fn analyze_signal(signal: &dsp::MonoSignal, options: &AnalysisOptions) -> TrackAnalysis {
    let envelopes = onset::onset_envelopes(signal);
    TrackAnalysis {
        tempo: tempo::estimate_tempo(&envelopes, options.min_bpm),
        key: key::estimate_key(signal),
    }
}
