//! Track analysis, run off the audio thread when a track is loaded.
//!
//! * [`waveform`]: peak and low / mid / high energy per time slice, for drawing
//!   colored waveforms; [`envelope`]: the height and colour drawn of each slice.
//! * [`tempo`]: BPM and beat grid, put on the kicks' attacks by [`grid`], on the beats and
//!   the bars a small neural network hears ([`beat_model`]), bar 1 where the track's sections
//!   start ([`sections`]).
//! * [`key`]: musical key, with its Camelot code.
//! * [`loudness`]: loudness (LUFS), to play tracks at the same level.
//! * [`stem_activity`]: where each part of a separated track plays.
//!
//! Tempo and key work on a mono copy decimated to ~11 kHz ([`dsp`]), and the tempo on
//! onset envelopes ([`onset`]).

pub mod beat_model;
pub mod dsp;
pub mod envelope;
pub mod grid;
pub mod key;
pub mod loudness;
pub mod onset;
mod priority;
pub mod sections;
pub mod stem_activity;
pub mod tempo;
pub mod waveform;

pub use key::{Key, KeyConfig, KeyEstimate, Mode, Profiles};
pub use loudness::{loudness, Loudness};
pub use priority::{analysis_priority, background_priority, full_speed, normal_priority, set_full_speed};
pub use stem_activity::{stem_activity, ACTIVITY_SLICES_PER_S};
pub use tempo::{octave_ratio, Tempo, DEFAULT_MIN_BPM};
pub use waveform::{compute_waveform, Waveform, WaveformBuilder};

use beat_model::{Excerpt, ExcerptTaker};

use prisme_media::{DecodeStream, MediaError, TrackAudio};
use std::path::Path;

/// Version of the tempo / key algorithms. Stored results from an older version are
/// recomputed: bump it whenever a change can alter them.
pub const ANALYSIS_VERSION: u32 = 7;

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

/// The beats of a bar, as electronic music counts them.
pub const BEATS_PER_BAR: u32 = 4;

/// Tempo and key are taken from the first this many seconds of a track: a set recorded for
/// an hour has no single tempo, and its whole signal would take hundreds of megabytes.
pub const MAX_ANALYSED_S: f64 = 20.0 * 60.0;

/// Tempo and key of a decoded track.
pub fn analyze(track: &TrackAudio, options: &AnalysisOptions) -> TrackAnalysis {
    let mut excerpts = ExcerptTaker::new(track.sample_rate(), Some(track.duration_s()));
    track.for_each_block(65_536, |block| excerpts.push(block));
    analyze_signal(&dsp::analysis_signal(track, MAX_ANALYSED_S), &excerpts.finish(), options)
}

/// Tempo and key of a file, decoded on the fly: memory stays at the size of the
/// decimated signal (~13 MB for 5 minutes) instead of the whole track. The calling thread gives
/// way to a deck that starts meanwhile (see [`analysis_priority`]).
pub fn analyze_file(path: &Path, options: &AnalysisOptions) -> Result<FileAnalysis, MediaError> {
    let mut stream = DecodeStream::open(path)?;
    let rate = f64::from(stream.sample_rate());
    let analysed = (MAX_ANALYSED_S * rate) as usize;
    let mut decimator = dsp::Decimator::new(stream.sample_rate());
    // past the limit, frames are decoded only to be counted: not when the header gives them
    let announced = stream.estimated_frames();
    let length = (announced > 0).then(|| (announced as f64 / rate).min(MAX_ANALYSED_S));
    let mut excerpts = ExcerptTaker::new(stream.sample_rate(), length);
    let (mut chunk, mut frames) = (Vec::new(), 0);
    while (frames < analysed || announced == 0) && stream.next_chunk(&mut chunk)? {
        analysis_priority();
        let room = 2 * analysed.saturating_sub(frames);
        decimator.push(&chunk[..chunk.len().min(room)]);
        excerpts.push(&chunk[..chunk.len().min(room)]);
        frames += chunk.len() / 2;
        chunk.clear();
    }
    let signal = decimator.finish();
    if signal.samples.is_empty() {
        return Err(MediaError::NoAudio);
    }
    let frames = if frames < analysed { frames } else { frames.max(announced) };
    let analysis = analyze_signal(&signal, &excerpts.finish(), options);
    Ok(FileAnalysis { duration_s: frames as f64 / rate, analysis })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FileAnalysis {
    pub duration_s: f64,
    pub analysis: TrackAnalysis,
}

fn analyze_signal(
    signal: &dsp::MonoSignal,
    excerpts: &[Excerpt],
    options: &AnalysisOptions,
) -> TrackAnalysis {
    let envelopes = onset::onset_envelopes(signal);
    let tempo = tempo::estimate_tempo(&envelopes, options.min_bpm).map(|onsets| {
        // every other excerpt first, the others if the bars are not clear in them
        let (first, then): (Vec<&Excerpt>, Vec<&Excerpt>) = if excerpts.len() > 1 {
            (excerpts.iter().skip(1).step_by(2).collect(), excerpts.iter().step_by(2).collect())
        } else {
            (excerpts.iter().collect(), Vec::new())
        };
        let mut heard = beat_model::hear(&first);
        let fitted = grid::fit_grid(signal, onsets);
        let fitted = heard_beats(onsets, &heard)
            .map(|phase_s| grid::fit_grid(signal, Tempo { first_beat_s: phase_s, ..onsets }))
            .filter(|heard| kicks(signal, heard) as f64 >= KICKS_KEPT * kicks(signal, &fitted) as f64)
            .unwrap_or(fitted);
        let fitted = grid::on_click(signal, fitted);
        if beat_model::bar_start(&heard, fitted.bpm, fitted.first_beat_s).is_none() {
            heard.extend(beat_model::hear(&then));
        }
        // the first kick, else the first sound: an intro's first notes or a riser before the
        // first bar may come a beat or two early
        let first_kick =
            grid::kick_attacks(signal, fitted.bpm, fitted.first_beat_s).first().map(|a| a.time_s);
        let music_start_s = first_kick.unwrap_or_else(|| envelopes.first_onset_s());
        let bars = on_bars(fitted, &heard, music_start_s);
        // an intro's first kicks may be hits off the bars: the sections know better
        let heard_on = beat_model::bar_start(&heard, bars.bpm, bars.first_beat_s).map(|beat| beat as usize);
        let said = sections::bar_one(signal, bars, heard_on);
        let downbeat_s = bars.first_beat_s + said.beat as f64 * bars.beat_period_s();
        Tempo { bars_sure: said.sure, ..first_bar(bars, downbeat_s, music_start_s) }
    });
    TrackAnalysis { tempo, key: key::estimate_key(signal) }
}

/// The onsets' grid is moved onto the beats the network hears where it hears beats there this
/// many times more surely than around the onsets' (within [`AROUND_ONSETS`] of a beat).
const HEARD_BETTER: f32 = 2.0;
const AROUND_ONSETS: f64 = 0.15;
/// ...and where that grid sits on this share of the kicks the onsets' grid sits on, at least:
/// a kick without an attack (a sub) may go unheard by the network, not by the signal. Not
/// more: the notes of a bass line on the off-beats pass for kicks on the onsets' grid.
const KICKS_KEPT: f64 = 0.5;

/// Where the network hears the beats of the onsets' grid, if it hears none around them: the
/// onsets of a bass line on the off-beats outweighed the kicks'.
fn heard_beats(onsets: Tempo, heard: &[beat_model::Heard]) -> Option<f64> {
    let phase_s = beat_model::beat_phase(heard, onsets.bpm)?;
    let period = onsets.beat_period_s();
    let steps = 30;
    let around = (-steps..=steps)
        .map(|i| onsets.first_beat_s + f64::from(i) / f64::from(steps) * AROUND_ONSETS * period)
        .map(|phase| beat_model::beat_score(heard, onsets.bpm, phase))
        .fold(0.0f32, f32::max);
    (beat_model::beat_score(heard, onsets.bpm, phase_s) >= HEARD_BETTER * around).then_some(phase_s)
}

/// The kicks a grid sits on.
fn kicks(signal: &dsp::MonoSignal, grid: &Tempo) -> usize {
    grid::kick_attacks(signal, grid.bpm, grid.first_beat_s).len()
}

/// The grid's first beat on bar 1: the first bar where the music starts (its first kick),
/// give or take half a beat. A track made to be mixed starts on a bar's first beat, unless the
/// network surely hears the bars start a beat before or after it. Two beats away, it is not
/// believed: it mistakes a bar's third beat for its first, sure of it all the same.
fn on_bars(grid: Tempo, heard: &[beat_model::Heard], music_start_s: f64) -> Tempo {
    let period = grid.beat_period_s();
    let start = grid.first_beat_s + ((music_start_s - grid.first_beat_s) / period).round() * period;
    let shift = beat_model::bar_start(heard, grid.bpm, start).filter(|&k| k % 2 == 1).unwrap_or(0);
    first_bar(grid, start + f64::from(shift) * period, music_start_s)
}

/// The grid's first beat on the first bar where the music starts, give or take half a beat,
/// a bar starting at `downbeat_s`.
fn first_bar(grid: Tempo, downbeat_s: f64, music_start_s: f64) -> Tempo {
    let period = grid.beat_period_s();
    let bar = f64::from(BEATS_PER_BAR) * period;
    let bars = ((music_start_s - period / 2.0 - downbeat_s) / bar).ceil();
    Tempo { first_beat_s: downbeat_s + bars * bar, ..grid }
}
