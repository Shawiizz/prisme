//! Bar 1 from the track's sections.
//!
//! Dance music changes on a bar's first beat: the kick comes back, a bass line starts. Each
//! beat is compared with the same beat of the bars before it and of the bars after it: whole
//! bars apart, the pattern inside the bar cancels out, whichever beat of the bar it is. Inside
//! a section a beat is like both. A section starts where the beats are like what follows and
//! unlike what came before; the fill before it is like neither. Each start votes for its beat
//! of the bar.

use crate::dsp::{MonoSignal, Stft};
use crate::tempo::Tempo;
use crate::BEATS_PER_BAR;

/// 46 ms frames every 12 ms (at 11 025 Hz).
const FRAME: usize = 512;
const HOP: usize = 128;
/// The bands a beat is heard in (Hz), and how much each weighs: the kick and the bass carry
/// the sections, voices and leads come in ahead of the bar.
const BANDS: [(f64, f64, f32); 7] = [
    (30.0, 90.0, 3.0),
    (90.0, 200.0, 3.0),
    (200.0, 500.0, 2.0),
    (500.0, 1_000.0, 1.0),
    (1_000.0, 2_000.0, 1.0),
    (2_000.0, 4_000.0, 1.0),
    (4_000.0, f64::INFINITY, 1.0),
];
/// A beat is its bands' levels over this many steps.
const STEPS: usize = 4;
/// A frame hears half its length either side of its centre: a step starts this early
/// (seconds), so that the attack on it is all its own.
const LEAD_S: f64 = 0.025;
/// Levels are counted from this far under the band's loudest step (dB): below is silence.
const RANGE_DB: f64 = 45.0;
/// A beat is compared with the same beat of this many bars before and after.
const BARS: usize = 2;
/// A section starts where a beat is this much further from the bars before than from the
/// bars after (dB, mean over its bands and steps)...
const START_DB: f32 = 2.0;
/// ...and goes on while the beats are half as much, over this many beats at least.
const MIN_BEATS: usize = 2;
/// Its first beat is the first, a bar back at most, from which the beats are no further from
/// the bars after than this many times what those of its first bar are: a beat the two
/// sections share is as much like one as like the other.
const LIKE_AFTER: f32 = 1.5;
/// Not where the level falls by more than this (dB, the bar from it against the bar before):
/// what stops does so ahead of the bar, on its last beats.
const MAX_FALL_DB: f32 = 3.0;
/// The most a start weighs: the track's first sound or its loudest drop does not outvote the
/// others.
const MAX_WEIGHT: f32 = 40.0;
/// Bar 1 moves to another beat of the bar that has this many times its votes...
const MOVES: f32 = 1.5;
/// ...and is sure with this many times the votes of any other beat.
const SURE: f32 = 1.5;

const BEATS: usize = BEATS_PER_BAR as usize;

/// What the sections say of the bars of a grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarOne {
    /// The beat of the grid's bar that bar 1 is on (0: where the grid has it).
    pub beat: usize,
    /// Whether the sections clearly say so.
    pub sure: bool,
}

/// Where the sections of the track put bar 1, `bars` the grid with the bar 1 found otherwise:
/// kept unless they clearly start on another beat of its bars.
pub fn bar_one(signal: &MonoSignal, bars: Tempo) -> BarOne {
    let votes = votes(&Levels::of(signal, bars));
    let most = (0..BEATS).max_by(|&a, &b| votes[a].total_cmp(&votes[b])).unwrap_or(0);
    let beat = if votes[most] > MOVES * votes[0] { most } else { 0 };
    let others = (0..BEATS).filter(|&b| b != beat).map(|b| votes[b]).fold(0.0, f32::max);
    BarOne { beat, sure: votes[beat] > 0.0 && votes[beat] >= SURE * others }
}

/// The weight of the section starts on each beat of the bar.
fn votes(levels: &Levels) -> [f32; BEATS] {
    let after: Vec<f32> = (0..levels.beats).map(|beat| levels.unlike(beat, Side::After)).collect();
    let lean: Vec<f32> =
        after.iter().enumerate().map(|(beat, after)| levels.unlike(beat, Side::Before) - after).collect();
    let mut votes = [0.0; BEATS];
    let mut beat = 1;
    while beat < lean.len() {
        if lean[beat] <= START_DB || lean[beat - 1] > START_DB {
            beat += 1;
            continue;
        }
        let end = (beat..lean.len()).find(|&b| lean[b] <= START_DB / 2.0).unwrap_or(lean.len());
        if end - beat >= MIN_BEATS {
            let bar = &after[beat..end.min(beat + BEATS)];
            let like = LIKE_AFTER * bar.iter().sum::<f32>() / bar.len() as f32;
            let back = beat.saturating_sub(BEATS - 1).max(1);
            let first = (back..beat).rev().find(|&b| after[b] > like).map_or(beat, |b| b + 1);
            let falls = levels.mean(first.saturating_sub(BEATS)..first) - levels.mean(first..first + BEATS);
            if falls <= MAX_FALL_DB {
                votes[first % BEATS] += lean[beat..end].iter().sum::<f32>().min(MAX_WEIGHT);
            }
        }
        beat = end;
    }
    votes
}

#[derive(Clone, Copy)]
enum Side {
    Before,
    After,
}

/// Per beat of the track (from the start of the grid's bar at or before 0), the weighted level
/// of each band over each step.
struct Levels {
    beats: usize,
    values: Vec<f32>,
}

impl Levels {
    const DIMS: usize = STEPS * BANDS.len();

    fn of(signal: &MonoSignal, bars: Tempo) -> Self {
        let rate = signal.sample_rate;
        let period = bars.beat_period_s();
        let bar = f64::from(BEATS_PER_BAR) * period;
        let start = bars.first_beat_s - (bars.first_beat_s / bar).ceil() * bar;
        let beats = ((signal.duration_s() - start) / period).max(0.0) as usize;
        let steps = beats * STEPS;
        let step = period / STEPS as f64;
        let bins: Vec<std::ops::Range<usize>> = BANDS
            .iter()
            .map(|&(low, high, _)| {
                let bin = |hz: f64| ((hz * FRAME as f64 / rate).ceil() as usize).min(FRAME / 2 + 1);
                bin(low)..bin(high)
            })
            .collect();

        let mut power = vec![0.0f64; steps * BANDS.len()];
        let mut frames = vec![0u32; steps];
        Stft::new(FRAME, HOP).for_each_frame(&signal.samples, |frame, magnitudes| {
            let centre = (frame * HOP + FRAME / 2) as f64 / rate;
            let at = ((centre - (start - LEAD_S)) / step).floor();
            if at < 0.0 || at >= steps as f64 {
                return;
            }
            let at = at as usize;
            frames[at] += 1;
            for (sum, bins) in power[at * BANDS.len()..].iter_mut().zip(&bins) {
                *sum += magnitudes[bins.clone()].iter().map(|&m| f64::from(m * m)).sum::<f64>();
            }
        });

        let weights: f32 = BANDS.iter().map(|b| b.2).sum();
        let mut values = vec![0.0f32; steps * BANDS.len()];
        for (band, &(_, _, weight)) in BANDS.iter().enumerate() {
            let db = |at: usize| {
                let mean = power[at * BANDS.len() + band] / f64::from(frames[at].max(1));
                10.0 * mean.max(f64::MIN_POSITIVE).log10()
            };
            let silence = (0..steps).map(db).fold(f64::MIN, f64::max) - RANGE_DB;
            let weight = weight * BANDS.len() as f32 / weights;
            for at in 0..steps {
                values[at * BANDS.len() + band] = (db(at) - silence).max(0.0) as f32 * weight;
            }
        }
        Self { beats, values }
    }

    fn beat(&self, beat: usize) -> &[f32] {
        &self.values[beat * Self::DIMS..(beat + 1) * Self::DIMS]
    }

    /// How far a beat is from the same beat of the bars on one side of it (silence past the
    /// ends of the track).
    fn unlike(&self, beat: usize, side: Side) -> f32 {
        let same: Vec<&[f32]> = (1..=BARS)
            .filter_map(|bar| match side {
                Side::Before => beat.checked_sub(bar * BEATS),
                Side::After => Some(beat + bar * BEATS).filter(|&b| b < self.beats),
            })
            .map(|b| self.beat(b))
            .collect();
        let apart = |(dim, &level): (usize, &f32)| {
            (level - same.iter().map(|beat| beat[dim]).sum::<f32>() / BARS as f32).abs()
        };
        self.beat(beat).iter().enumerate().map(apart).sum::<f32>() / Self::DIMS as f32
    }

    /// The mean level over some beats (those of the track).
    fn mean(&self, beats: std::ops::Range<usize>) -> f32 {
        let beats = beats.start..beats.end.min(self.beats);
        let values = &self.values[beats.start * Self::DIMS..beats.end * Self::DIMS];
        values.iter().sum::<f32>() / values.len().max(1) as f32
    }
}
