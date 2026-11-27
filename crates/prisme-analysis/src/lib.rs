//! Track analysis, run off the audio thread when a track is loaded.
//!
//! * [`waveform`]: peak and low / mid / high energy per time slice, for drawing
//!   colored waveforms; [`envelope`]: the height and colour drawn of each slice.
//! * [`tempo`]: BPM and beat grid, put on the kicks' attacks by [`grid`], on the beats and
//!   the bars a small neural network hears ([`beat_model`]), bar 1 where the track's sections
//!   start ([`sections`]), pinned on the kicks of a track whose tempo drifts ([`drift`]).
//! * [`key`]: musical key, with its Camelot code.
//! * [`loudness`]: loudness (LUFS), to play tracks at the same level.
//! * [`stem_activity`]: where each part of a separated track plays.
//!
//! Tempo and key work on a mono copy decimated to ~11 kHz ([`dsp`]), and the tempo on
//! onset envelopes ([`onset`]).

pub mod beat_model;
pub mod drift;
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
pub use priority::{
    analysis_priority, background_priority, for_a_deck, full_speed, normal_priority, set_full_speed,
};
pub use stem_activity::{stem_activity, ACTIVITY_SLICES_PER_S};
pub use tempo::{octave_ratio, Tempo, DEFAULT_MIN_BPM};
pub use waveform::{compute_waveform, Waveform, WaveformBuilder};

use beat_model::{Excerpt, ExcerptTaker};

use prisme_grid::{Anchor, BeatGrid};
use prisme_media::{DecodeStream, MediaError, TrackAudio};
use std::path::Path;
use std::sync::Arc;

/// Version of the tempo / key algorithms. Stored results from an older version are
/// recomputed: bump it whenever a change can alter them.
pub const ANALYSIS_VERSION: u32 = 8;

#[derive(Debug, Clone, Copy)]
pub struct AnalysisOptions {
    /// The BPM is reported in `[min_bpm, 2 * min_bpm)`.
    pub min_bpm: f64,
    /// The threads the neural network runs on: one for the tracks of a library, analysed side
    /// by side; [`awaited_threads`] for a track the DJ waits for.
    pub network_threads: usize,
}

impl Default for AnalysisOptions {
    fn default() -> Self {
        Self { min_bpm: DEFAULT_MIN_BPM, network_threads: 1 }
    }
}

/// The threads the neural network runs on for a track analysed alone, the DJ waiting for it:
/// half the cores, four at most (more gain it little).
pub fn awaited_threads() -> usize {
    std::thread::available_parallelism().map_or(1, |cores| (cores.get() / 2).clamp(1, 4))
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackAnalysis {
    /// Its tempo, on average from the first anchor to the last if it drifts.
    pub tempo: Option<Tempo>,
    pub key: Option<KeyEstimate>,
    /// Where its beats are pinned, a track whose tempo drifts (see [`drift`]); none for one tempo
    /// throughout.
    pub anchors: Option<Arc<[Anchor]>>,
}

impl TrackAnalysis {
    /// Its beat grid: on its anchors, if its tempo drifts.
    pub fn beat_grid(&self) -> Option<BeatGrid> {
        let tempo = self.tempo?;
        let pinned = self.anchors.as_deref().and_then(|anchors| {
            let [.., before, last] = anchors else { return None };
            let after = (last.beat - before.beat) / (last.time_s - before.time_s) * 60.0;
            BeatGrid::anchored(anchors.to_vec(), after, tempo.bars_sure)
        });
        Some(pinned.unwrap_or_else(|| BeatGrid::steady(tempo.bpm, tempo.first_beat_s, tempo.bars_sure)))
    }
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

#[derive(Debug, Clone, PartialEq)]
pub struct FileAnalysis {
    pub duration_s: f64,
    pub analysis: TrackAnalysis,
}

fn analyze_signal(
    signal: &dsp::MonoSignal,
    excerpts: &[Excerpt],
    options: &AnalysisOptions,
) -> TrackAnalysis {
    // between its steps, it gives way to a deck that starts or waits for its own track
    let envelopes = onset::onset_envelopes(signal);
    analysis_priority();
    let mut attacks = Vec::new();
    let tempo = tempo::estimate_tempo(&envelopes, options.min_bpm).map(|onsets| {
        let (first, then) = where_the_kick_plays(excerpts, &envelopes);
        let mut heard = beat_model::hear(&first, options.network_threads);
        analysis_priority();
        let fitted = grid::fit_grid(signal, onsets);
        let on_onsets = grid::on_click(signal, fitted);
        let fitted = heard_beats(onsets, &heard)
            .map(|phase_s| grid::fit_grid(signal, Tempo { first_beat_s: phase_s, ..onsets }))
            .filter(|heard| kicks(signal, heard) as f64 >= KICKS_KEPT * kicks(signal, &fitted) as f64)
            .map(|heard| grid::on_click(signal, heard))
            .filter(|heard| another_pulse(heard, &on_onsets))
            .unwrap_or(on_onsets);
        if beat_model::bar_start(&heard, fitted.bpm, fitted.first_beat_s).is_none() {
            heard.extend(beat_model::hear(&then, options.network_threads));
        }
        // the first kick, else the first sound: an intro's first notes or a riser before the
        // first bar may come a beat or two early
        attacks =
            grid::kick_attacks(signal, fitted.bpm, fitted.first_beat_s).iter().map(|a| a.time_s).collect();
        let first_kick = attacks.first().copied();
        let music_start_s = first_kick.unwrap_or_else(|| envelopes.first_onset_s());
        analysis_priority();
        let bars = on_bars(fitted, &heard, music_start_s);
        // an intro's first kicks may be hits off the bars: the sections know better
        let heard_on = beat_model::bar_start(&heard, bars.bpm, bars.first_beat_s).map(|beat| beat as usize);
        let said = sections::bar_one(signal, bars, heard_on);
        let downbeat_s = bars.first_beat_s + said.beat as f64 * bars.beat_period_s();
        Tempo { bars_sure: said.sure, ..first_bar(bars, downbeat_s, music_start_s) }
    });
    analysis_priority();
    // a tempo that drifts: the grid pinned on the kicks, its tempo theirs on average
    let pinned = tempo.and_then(|steady| {
        let anchors = drift::follow(signal, &steady, &attacks).anchors?;
        let grid = BeatGrid::anchored(anchors.clone(), steady.bpm, steady.bars_sure)?;
        let bpm = grid.mean_bpm();
        Some((Tempo { bpm, first_beat_s: grid.first_beat_s(), ..steady }, Arc::from(anchors)))
    });
    let (tempo, anchors) = match pinned {
        Some((tempo, anchors)) => (Some(tempo), Some(anchors)),
        None => (tempo, None),
    };
    TrackAnalysis { tempo, key: key::estimate_key(signal), anchors }
}

/// Excerpts the network hears first: one tells where the beats are, the bars take three...
const HEARD_FIRST: usize = 3;
/// ...and as many more if they are not clear in them.
const HEARD_THEN: usize = 3;

/// The excerpts the network hears, first and then: those where the bass strikes most, where the
/// kick plays, not a breakdown or an intro that has nothing to tell of the beat.
fn where_the_kick_plays<'a>(
    excerpts: &'a [Excerpt],
    envelopes: &onset::OnsetEnvelopes,
) -> (Vec<&'a Excerpt>, Vec<&'a Excerpt>) {
    let mut most: Vec<(f32, &Excerpt)> =
        excerpts.iter().map(|e| (envelopes.low_between(e.start_s, e.end_s()), e)).collect();
    most.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut most = most.into_iter().map(|(_, excerpt)| excerpt);
    let first = most.by_ref().take(HEARD_FIRST).collect();
    (first, most.take(HEARD_THEN).collect())
}

/// The onsets' grid is moved onto the beats the network hears where it hears beats there this
/// many times more surely than around the onsets' (within [`AROUND_ONSETS`] of a beat)...
const HEARD_BETTER: f32 = 2.0;
const AROUND_ONSETS: f64 = 0.15;
/// ...and where that grid sits on this share of the kicks the onsets' grid sits on, at least:
/// a kick without an attack (a sub) may go unheard by the network, not by the signal. Not
/// more: the notes of a bass line on the off-beats pass for kicks on the onsets' grid.
const KICKS_KEPT: f64 = 0.5;

/// ...and where the two, each put on its kicks, are this far apart (beats): nearer, they are
/// one pulse, which the network hears a little late (the swell of a kick without an attack)
/// and the signal places better.
const ANOTHER_PULSE: f64 = 0.25;

/// Whether two grids of a tempo are on different pulses, not one placed a little apart.
fn another_pulse(grid: &Tempo, other: &Tempo) -> bool {
    let period = grid.beat_period_s();
    let apart = (grid.first_beat_s - other.first_beat_s).rem_euclid(period);
    apart.min(period - apart) > ANOTHER_PULSE * period
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_network_hears_the_excerpts_where_the_kick_plays() {
        // ten frames a second; the bass strikes from 100 s to 200 s
        let low = (0..3000).map(|frame| if (1000..2000).contains(&frame) { 1.0 } else { 0.0 }).collect();
        let envelopes = onset::OnsetEnvelopes { full: vec![0.0; 3000], low, frame_rate: 10.0, start_s: 0.0 };
        let excerpts: Vec<Excerpt> = (0..10)
            .map(|i| Excerpt { start_s: 30.0 * f64::from(i), sample_rate: 10, samples: vec![0.0; 150] })
            .collect();
        let (first, then) = where_the_kick_plays(&excerpts, &envelopes);
        let starts = |excerpts: &[&Excerpt]| excerpts.iter().map(|e| e.start_s).collect::<Vec<_>>();
        assert_eq!(starts(&first), [120.0, 150.0, 180.0]);
        assert_eq!(starts(&then)[0], 90.0, "the one it plays in part of");
    }
}
