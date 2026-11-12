//! Track analysis, run off the audio thread when a track is loaded.
//!
//! * [`waveform`]: amplitude and low / mid / high energy per time slice, for drawing
//!   colored waveforms.
//! * [`tempo`]: BPM and beat grid.
//! * [`key`]: musical key, with its Camelot code.
//! * [`loudness`]: loudness (LUFS), to play tracks at the same level.
//! * [`stem_activity`]: where each part of a separated track plays.
//!
//! Tempo and key work on a mono copy decimated to ~11 kHz ([`dsp`]), and the tempo on
//! onset envelopes ([`onset`]).

pub mod dsp;
pub mod key;
pub mod loudness;
pub mod onset;
pub mod stem_activity;
pub mod tempo;
pub mod waveform;

pub use key::{Key, KeyConfig, KeyEstimate, Mode, Profiles};
pub use loudness::{loudness, Loudness};
pub use stem_activity::{stem_activity, ACTIVITY_SLICES_PER_S};
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

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TrackAnalysis {
    pub tempo: Option<Tempo>,
    pub key: Option<KeyEstimate>,
}

/// Tempo and key are taken from the first this many seconds of a track: a set recorded for
/// an hour has no single tempo, and its whole signal would take hundreds of megabytes.
pub const MAX_ANALYSED_S: f64 = 20.0 * 60.0;

/// Tempo and key of a decoded track.
pub fn analyze(track: &TrackAudio, options: &AnalysisOptions) -> TrackAnalysis {
    analyze_signal(&dsp::analysis_signal(track, MAX_ANALYSED_S), options)
}

/// Tempo and key of a file, decoded on the fly: memory stays at the size of the
/// decimated signal (~13 MB for 5 minutes) instead of the whole track.
pub fn analyze_file(path: &Path, options: &AnalysisOptions) -> Result<FileAnalysis, MediaError> {
    let mut stream = DecodeStream::open(path)?;
    let rate = f64::from(stream.sample_rate());
    let analysed = (MAX_ANALYSED_S * rate) as usize;
    let mut decimator = dsp::Decimator::new(stream.sample_rate());
    let (mut chunk, mut frames) = (Vec::new(), 0);
    while stream.next_chunk(&mut chunk)? {
        let room = 2 * analysed.saturating_sub(frames);
        decimator.push(&chunk[..chunk.len().min(room)]);
        frames += chunk.len() / 2;
        chunk.clear();
    }
    let signal = decimator.finish();
    if signal.samples.is_empty() {
        return Err(MediaError::NoAudio);
    }
    Ok(FileAnalysis { duration_s: frames as f64 / rate, analysis: analyze_signal(&signal, options) })
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
