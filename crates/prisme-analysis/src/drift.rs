//! Beat grids whose tempo drifts: a track played by a band, a live drummer, a record of the
//! seventies. The steady grid of [`crate::tempo`] and [`crate::grid`] sits on the kicks on
//! average; here the kicks are followed along the whole track, and where they stray from the
//! steady grid it is pinned on them by anchors, its tempo steady between two. A track made in a
//! DAW keeps its steady grid untouched.
//!
//! 1. The track's own kick is pictured, in two bands: its thump below 150 Hz, its click from 1 to
//!    5 kHz, each the envelope around the kicks the steady grid sits on, averaged. How alike each
//!    instant of the track is to both (correlations through the whole track at once) tells where
//!    that kick strikes: a bass note on the off-beat only thumps, a hat or a snare only clicks.
//! 2. The beats are the path through that likeness that lands on the most kicks while keeping as
//!    steady a tempo as it can from one beat to the next (dynamic programming, as Ellis's beat
//!    tracker), and as near the steady grid's beats: weighed whole, it follows a tempo that
//!    drifts, goes through a breakdown at the tempo it had, and leaves the steady grid only for
//!    kicks surely elsewhere.
//! 3. The kicks of the path come in runs between breakdowns, each numbered on its own, then from
//!    one to the next as their drifts go on either side.
//! 4. How far the kicks stray within a run, a median over a few bars at a time, says whether the
//!    tempo drifts: between two runs the kick's sound may change, and the place its likeness
//!    peaks with it. If it does, the fewest anchors put the grid within a few milliseconds of the
//!    kicks, as far from them as the steady grid is (on their click).

use crate::dsp::MonoSignal;
use crate::tempo::Tempo;
use prisme_grid::{Anchor, BeatGrid};
use realfft::RealFftPlanner;
use std::f64::consts::TAU;

/// The kick's envelope is taken every this many samples of the signal...
const DECIMATION: usize = 16;
/// ...in two bands: its body, below this (Hz), and its click, between these...
const LOW_BAND_HZ: f64 = 150.0;
const CLICK_BAND_HZ: (f64, f64) = (1_000.0, 5_000.0);
/// ...smoothed over this (seconds).
const SMOOTHING_S: f64 = 0.001;
/// The picture of the kick: from this long before its attack to this long after (seconds).
const PICTURE_S: (f64, f64) = (0.03, 0.06);
/// Positions of the likeness to the kick looked at as one by the path.
const FRAME: usize = 2;
/// How far apart two beats of the path may be, against the steady grid's beat.
const SHORTEST: f64 = 0.75;
const LONGEST: f64 = 1.33;
/// What a beat of the path costs for not lasting the steady grid's beat: this much times the
/// square of the logarithm of their ratio, against likenesses whose spread is 1.
const TIGHTNESS: f64 = 400.0;
/// What a beat of the path half a beat off the steady grid's costs, against likenesses whose
/// spread is 1.
const PRIOR: f64 = 40.0;
/// A beat of the path holds the kick where the likeness there is this much at least.
const KICK_LIKENESS: f32 = 0.3;
/// The kicks' drift goes on as it went over the last this many kicks, at most this much a beat
/// (beats).
const TREND_KICKS: usize = 16;
const MAX_TREND: f64 = 0.05;
/// Kicks further apart than this many beats have a breakdown between them; fewer kicks than this
/// between two breakdowns do not count.
const GAP_BEATS: f64 = 8.0;
const RUN_MIN_KICKS: usize = 16;
/// A kick counts among this many others at least, within this many beats of it, each a whole
/// number of beats apart within this much (beats).
const RUN_KICKS: usize = 2;
const RUN_BEATS: f64 = 2.5;
const ON_BEAT: f64 = 0.1;
/// A kick further than this from the beat where the drift puts it is not the kick (beats).
const OFF_BEAT: f64 = 0.2;
/// A kick this near a beat of the steady grid is on it (beats).
const ON_GRID: f64 = 0.05;
/// The kicks are compared this many beats at a time (a median)...
const STRETCH_BEATS: i64 = 32;
/// ...those of a stretch with this many kicks at least.
const STRETCH_KICKS: usize = 8;
/// The tempo drifts where the kicks stray this far within a run of them (seconds): a drummer's
/// sway, further than the kick's sound moves the place its likeness peaks from one section of a
/// track made in a DAW to another (a filter opening on it, a layer added).
pub const DRIFT_S: f64 = 0.030;
/// The kicks are smoothed over this many (a median) for the anchors: one kick played early, a
/// flam, do not move the grid.
const SMOOTHING: usize = 9;
/// The kicks struck sit this much closer to the beats pinned than to the steady grid's at most
/// (their spread around them).
const CLOSER: f64 = 0.5;
/// The anchors keep the grid this close to the smoothed kicks (seconds).
const FIT_S: f64 = 0.003;
/// Fewer kicks than this and the tempo is taken as steady.
const MIN_KICKS: usize = 32;

/// A beat of the track and where it falls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Beat {
    /// Counted from bar 1's first on the steady grid.
    pub beat: i64,
    pub time_s: f64,
}

/// What following the kicks of a track tells.
#[derive(Debug, Clone, PartialEq)]
pub struct Drift {
    /// The beats of the path that hold the kick, where it strikes.
    pub kicks: Vec<Beat>,
    /// How far they stray within a run of kicks, from one stretch of it to another, where they
    /// stray most (seconds).
    pub straying_s: f64,
    /// Anchors pinning the grid on the kicks, if the tempo drifts.
    pub anchors: Option<Vec<Anchor>>,
}

/// The kicks of a track followed from its steady grid `grid` and the attacks of the kicks it sits
/// on (`attacks`, seconds), and the anchors that pin the grid on them if its tempo drifts.
pub fn follow(signal: &MonoSignal, grid: &Tempo, attacks: &[f64]) -> Drift {
    let none = |kicks| Drift { kicks, straying_s: 0.0, anchors: None };
    if attacks.len() < MIN_KICKS {
        return none(Vec::new());
    }
    let likeness = Likeness::of(signal, attacks);
    // the path's beats are where the kicks' attacks are, a little before the steady grid's on
    // their click
    let period = grid.beat_period_s();
    let phase =
        median(attacks.iter().map(|&a| a - ((a - grid.first_beat_s) / period).round() * period).collect());
    let on_attacks = Tempo { first_beat_s: phase, ..*grid };
    let struck = likeness.path(&on_attacks);
    let kicks = numbered(&struck, &on_attacks);
    if kicks.len() < MIN_KICKS {
        return none(kicks);
    }
    let beat_s = |beat: i64| grid.first_beat_s + beat as f64 * period;
    let offsets: Vec<f64> = kicks.iter().map(|k| k.time_s - beat_s(k.beat)).collect();
    let straying_s = straying_within_runs(&kicks, &offsets);
    if straying_s < DRIFT_S {
        return Drift { kicks, straying_s, anchors: None };
    }
    // as far from the kicks as the steady grid is from its own: on their click
    let click_s = median(
        attacks.iter().map(|&a| beat_s(((a - grid.first_beat_s) / period).round() as i64) - a).collect(),
    );
    let smoothed = running_median(&offsets, SMOOTHING);
    let points: Vec<(f64, f64)> =
        kicks.iter().zip(&smoothed).map(|(k, o)| (k.beat as f64, beat_s(k.beat) + o + click_s)).collect();
    let anchors: Vec<Anchor> =
        fewest_anchors(&points, FIT_S).into_iter().map(|(beat, time_s)| Anchor { beat, time_s }).collect();
    // the kicks struck, all of them, sit closer to the beats pinned than to the steady grid's: a
    // run numbered wrong would put whole stretches between two beats
    let steady = BeatGrid::steady(grid.bpm, grid.first_beat_s, true);
    let closer = match anchors.as_slice() {
        [.., before, last] => {
            let after = (last.beat - before.beat) / (last.time_s - before.time_s) * 60.0;
            BeatGrid::anchored(anchors.clone(), after, true)
                .is_some_and(|pinned| spread(&struck, &pinned) <= CLOSER * spread(&struck, &steady))
        }
        _ => false,
    };
    Drift { kicks, straying_s, anchors: closer.then_some(anchors) }
}

/// How alike each instant of a track is to its kick, every [`DECIMATION`] samples of the signal.
struct Likeness {
    values: Vec<f32>,
    rate: f64,
    /// Where the attack is in the picture (positions after its start).
    attack: usize,
}

impl Likeness {
    /// The likeness of `signal` to the kick struck at `attacks` (seconds): the product of its
    /// likeness in the two bands (see [`band_likeness`]), or the square of the one there is.
    fn of(signal: &MonoSignal, attacks: &[f64]) -> Self {
        let rate = signal.sample_rate / DECIMATION as f64;
        let position = |s: f64| (s * rate).round() as isize;
        let (before, length) = (position(PICTURE_S.0) as usize, position(PICTURE_S.0 + PICTURE_S.1) as usize);
        let starts: Vec<isize> = attacks.iter().map(|&a| position(a) - before as isize).collect();
        // a kick thumps low and clicks high at once: a bass note only thumps, a hat or a snare
        // only clicks
        let low = band_likeness(&envelope(signal, None, LOW_BAND_HZ), &starts, length);
        let click = band_likeness(&envelope(signal, Some(CLICK_BAND_HZ.0), CLICK_BAND_HZ.1), &starts, length);
        let values = match (low, click) {
            (Some(low), Some(click)) => low.iter().zip(&click).map(|(l, c)| l * c).collect(),
            (Some(only), None) | (None, Some(only)) => only.iter().map(|v| v * v).collect(),
            _ => Vec::new(),
        };
        Self { values, rate, attack: before }
    }

    /// When the attack of a kick matched at position `i` strikes.
    fn time_s(&self, i: f64) -> f64 {
        (i + self.attack as f64) / self.rate
    }

    /// Where the kick strikes on the beats of the path through the likeness that lands on the
    /// most kicks with the steadiest tempo, near the beats of `grid`.
    fn path(&self, grid: &Tempo) -> Vec<f64> {
        let frames: Vec<f32> =
            self.values.chunks(FRAME).map(|c| c.iter().copied().fold(0.0, f32::max)).collect();
        let n = frames.len();
        let period = grid.beat_period_s() * self.rate / FRAME as f64;
        let (shortest, longest) = ((SHORTEST * period).floor() as usize, (LONGEST * period).ceil() as usize);
        if n <= longest || shortest == 0 {
            return Vec::new();
        }
        let mean = frames.iter().map(|&v| f64::from(v)).sum::<f64>() / n as f64;
        let spread = (frames.iter().map(|&v| (f64::from(v) - mean).powi(2)).sum::<f64>() / n as f64).sqrt();
        if spread <= 0.0 {
            return Vec::new();
        }
        let cost: Vec<f64> =
            (0..=longest).map(|d| TIGHTNESS * (d.max(1) as f64 / period).ln().powi(2)).collect();
        // off the steady grid's beats, a beat costs as the square of how far (half a beat off at
        // most): the path leaves them only for kicks that are surely elsewhere, not for a sound on
        // the off-beat as like the kick as the kick on the beat
        let off = |i: usize| {
            let x = (self.time_s((i * FRAME + FRAME / 2) as f64) - grid.first_beat_s) / grid.beat_period_s();
            PRIOR * (x - x.round()).powi(2)
        };
        let mut score = vec![0.0f64; n];
        let mut from = vec![usize::MAX; n];
        for i in 0..n {
            // a path starts within its first beat, then goes on from beat to beat to the end
            let mut best = if i < longest { (0.0, usize::MAX) } else { (f64::NEG_INFINITY, usize::MAX) };
            for d in shortest..=longest.min(i) {
                let s = score[i - d] - cost[d];
                if s > best.0 {
                    best = (s, i - d);
                }
            }
            score[i] = f64::from(frames[i]) / spread - off(i) + best.0;
            from[i] = best.1;
        }
        let last = (n - longest..n).max_by(|&a, &b| score[a].total_cmp(&score[b])).unwrap_or(n - 1);
        let mut path = vec![last];
        while let Some(&i) = path.last().filter(|&&i| from[i] != usize::MAX) {
            path.push(from[i]);
        }
        path.reverse();
        // each beat: the kick where the likeness peaks in its frame and those beside it
        let beats: Vec<Option<f64>> = path
            .iter()
            .map(|&f| {
                let range = (f * FRAME).saturating_sub(FRAME)..((f + 2) * FRAME).min(self.values.len());
                let best = range.clone().max_by(|&a, &b| self.values[a].total_cmp(&self.values[b]))?;
                if self.values[best] < KICK_LIKENESS {
                    return None;
                }
                let at = |i: usize| self.values.get(i).copied().unwrap_or(0.0);
                let (l, c, r) = (at(best.wrapping_sub(1)), at(best), at(best + 1));
                let denom = l - 2.0 * c + r;
                let delta = if denom < 0.0 { f64::from(0.5 * (l - r) / denom).clamp(-0.5, 0.5) } else { 0.0 };
                Some(self.time_s(best as f64 + delta))
            })
            .collect();
        beats.into_iter().flatten().collect()
    }
}

/// The kicks at `times`, in order, numbered as the beats of the steady grid `grid`: run by run,
/// each kick the beat that keeps the run's drift going on as it went, then the run with the most
/// kicks on the grid's beats numbered as them, and each other from the one beside it. The path's
/// own count would not do: through a breakdown with no kick, it may squeeze a beat in or leave
/// one out at a cost too small to tell.
fn numbered(times: &[f64], grid: &Tempo) -> Vec<Beat> {
    let all: Vec<f64> = times.iter().map(|&t| (t - grid.first_beat_s) / grid.beat_period_s()).collect();
    // a kick among kicks a whole number of beats apart: alone in a breakdown, a sound that looks
    // like it is not one
    let in_run = |j: usize| {
        all.iter()
            .enumerate()
            .filter(|&(i, &x)| {
                let apart = (x - all[j]).abs();
                i != j && apart <= RUN_BEATS && (apart - apart.round()).abs() <= ON_BEAT
            })
            .count()
            >= RUN_KICKS
    };
    let (times, steady): (Vec<f64>, Vec<f64>) =
        (0..all.len()).filter(|&j| in_run(j)).map(|j| (times[j], all[j])).unzip();
    // runs of kicks without a breakdown between two: each numbered on its own, from its first
    let mut runs: Vec<Run> = Vec::new();
    for (&time_s, &x) in times.iter().zip(&steady) {
        match runs.last_mut().filter(|run| run.kicks.last().is_some_and(|k| x - k.x <= GAP_BEATS)) {
            Some(run) => run.add(time_s, x),
            None => runs.push(Run { kicks: vec![RunKick { time_s, x, beat: 0 }] }),
        }
    }
    // the run with the most kicks on the steady grid's beats numbered as them; each other from
    // the one beside it, its drift going on through the breakdown between as the two runs' drifts
    // go on either side
    let on_grid = |run: &Run| {
        let drifts = run.drifts();
        let near: Vec<f64> =
            drifts.iter().map(|d| d.1).filter(|d| (d - d.round()).abs() <= ON_GRID).collect();
        (near.len(), near.first().map_or(0, |d| d.round() as i64))
    };
    // a few kicks alone between two breakdowns: too few to tell where the beats are
    runs.retain(|run| run.kicks.len() >= RUN_MIN_KICKS);
    let Some(reference) =
        (0..runs.len()).filter(|&r| on_grid(&runs[r]).0 > 0).max_by_key(|&r| on_grid(&runs[r]).0)
    else {
        return Vec::new();
    };
    // a run whose drift is off the way the others go by part of a beat is on another pulse: a
    // sound on the off-beat that looks like the kick, in a section without it
    let on_beat = |drift: f64| (drift - drift.round()).abs() <= OFF_BEAT;
    let mut shifts: Vec<Option<i64>> = vec![None; runs.len()];
    shifts[reference] = Some(on_grid(&runs[reference]).1);
    let mut kept = reference;
    for r in reference + 1..runs.len() {
        let (before, after) = (&runs[kept], &runs[r]);
        let (x0, d0) = *before.drifts().last().expect("a run has kicks");
        let (x1, d1) = after.drifts()[0];
        let trend = (before.trend() + after.trend()) / 2.0;
        let off = d1 - (d0 - shifts[kept].unwrap_or(0) as f64 + trend * (x1 - x0));
        if on_beat(off) {
            shifts[r] = Some(off.round() as i64);
            kept = r;
        }
    }
    kept = reference;
    for r in (0..reference).rev() {
        let (before, after) = (&runs[r], &runs[kept]);
        let (x0, d0) = *before.drifts().last().expect("a run has kicks");
        let (x1, d1) = after.drifts()[0];
        let trend = (before.trend() + after.trend()) / 2.0;
        let off = d0 - (d1 - shifts[kept].unwrap_or(0) as f64 - trend * (x1 - x0));
        if on_beat(off) {
            shifts[r] = Some(off.round() as i64);
            kept = r;
        }
    }
    let mut numbered: Vec<Beat> = runs
        .iter()
        .zip(&shifts)
        .filter_map(|(run, shift)| shift.map(|shift| (run, shift)))
        .flat_map(|(run, shift)| {
            run.kicks.iter().map(move |k| Beat { beat: k.beat + shift, time_s: k.time_s })
        })
        .collect();
    numbered.dedup_by_key(|b| b.beat);
    numbered
}

/// Kicks one after the other, no more than [`GAP_BEATS`] apart, numbered from the first.
struct Run {
    kicks: Vec<RunKick>,
}

/// A kick of a run: where it strikes, where that is on the steady grid (beats), its beat in the
/// run.
struct RunKick {
    time_s: f64,
    x: f64,
    beat: i64,
}

impl Run {
    /// Adds the next kick, the beat that keeps the run's drift going on as it went: none if it
    /// falls between two beats.
    fn add(&mut self, time_s: f64, x: f64) {
        let drifts = self.drifts();
        let recent = &drifts[drifts.len().saturating_sub(TREND_KICKS)..];
        let (xs, ys): (Vec<f64>, Vec<f64>) = recent.iter().copied().unzip();
        let slope =
            if recent.len() >= 4 { line_through(&xs, &ys).1.clamp(-MAX_TREND, MAX_TREND) } else { 0.0 };
        let &(last_x, last_drift) = drifts.last().expect("a run has kicks");
        let expected = last_drift + slope * (x - last_x);
        let beat = (x - expected).round();
        if (x - beat - expected).abs() <= OFF_BEAT {
            self.kicks.push(RunKick { time_s, x, beat: beat as i64 });
        }
    }

    /// Each kick's place on the steady grid and its drift from its beat in the run (beats).
    fn drifts(&self) -> Vec<(f64, f64)> {
        self.kicks.iter().map(|k| (k.x, k.x - k.beat as f64)).collect()
    }

    /// How the drift goes along the run (beats a beat): through the whole of it, its ends alone
    /// being too few kicks to tell, against the breakdowns around.
    fn trend(&self) -> f64 {
        let (xs, ys): (Vec<f64>, Vec<f64>) = self.drifts().into_iter().unzip();
        line_through(&xs, &ys).1.clamp(-MAX_TREND, MAX_TREND)
    }
}

/// How alike each stretch `length` long of `envelope` is to its stretches starting at `starts`
/// (those of the kicks), averaged: their correlation (none below 0), weighed by how loud the
/// stretch is against the kicks, up to theirs. `None` without kicks enough, or a flat picture.
fn band_likeness(envelope: &[f32], starts: &[isize], length: usize) -> Option<Vec<f32>> {
    let mut picture = vec![0.0f64; length];
    let mut levels = Vec::with_capacity(starts.len());
    for window in starts
        .iter()
        .filter_map(|&s| envelope.get(usize::try_from(s).ok()?..usize::try_from(s).ok()? + length))
    {
        let level = window.iter().map(|&v| f64::from(v)).sum::<f64>() / length as f64;
        if level > 0.0 {
            levels.push(level);
            for (p, &v) in picture.iter_mut().zip(window) {
                *p += f64::from(v) / level;
            }
        }
    }
    let n = envelope.len();
    if levels.len() < MIN_KICKS || n < length {
        return None;
    }
    let kick_level = median(levels.clone());
    let mean = picture.iter().sum::<f64>() / length as f64;
    let picture: Vec<f64> = picture.iter().map(|p| p - mean).collect();
    let picture_norm = picture.iter().map(|p| p * p).sum::<f64>().sqrt();
    if picture_norm <= 0.0 {
        return None;
    }
    let dots = correlation(envelope, &picture);
    let mut sums = vec![(0.0f64, 0.0f64); n + 1];
    for (i, &v) in envelope.iter().enumerate() {
        let v = f64::from(v);
        sums[i + 1] = (sums[i].0 + v, sums[i].1 + v * v);
    }
    Some(
        (0..=n - length)
            .map(|i| {
                let (s1, s2) = (sums[i + length].0 - sums[i].0, sums[i + length].1 - sums[i].1);
                let spread = (s2 - s1 * s1 / length as f64).max(0.0).sqrt();
                if spread <= 0.0 {
                    return 0.0;
                }
                let r = (dots[i] / (spread * picture_norm)).max(0.0);
                let loud = (s1 / length as f64 / kick_level).min(1.0);
                (r * loud.sqrt()) as f32
            })
            .collect(),
    )
}

/// The envelope of the signal above `low_hz` (if any) and below `high_hz`, every
/// [`DECIMATION`] samples.
fn envelope(signal: &MonoSignal, low_hz: Option<f64>, high_hz: f64) -> Vec<f32> {
    let rate = signal.sample_rate;
    let pole = |hz: f64| (-TAU * hz / rate).exp() as f32;
    let (high, low) = (pole(high_hz), low_hz.map(pole));
    let smooth = (-1.0 / (SMOOTHING_S * rate)).exp() as f32;
    let (mut a, mut b, mut c, mut d, mut power) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let mut envelope = Vec::with_capacity(signal.samples.len() / DECIMATION + 1);
    for (i, &x) in signal.samples.iter().enumerate() {
        a = (1.0 - high) * x + high * a;
        b = (1.0 - high) * a + high * b;
        let y = match low {
            // twice the low pass taken off: what is above it
            Some(low) => {
                c = (1.0 - low) * b + low * c;
                d = (1.0 - low) * (b - c) + low * d;
                b - c - d
            }
            None => b,
        };
        power = (1.0 - smooth) * y * y + smooth * power;
        if i % DECIMATION == 0 {
            envelope.push(power.sqrt());
        }
    }
    envelope
}

/// The dot product of `pattern` with each stretch of `values` as long as it, from each start.
fn correlation(values: &[f32], pattern: &[f64]) -> Vec<f64> {
    let size = (values.len() + pattern.len()).next_power_of_two();
    let mut planner = RealFftPlanner::<f32>::new();
    let (forward, inverse) = (planner.plan_fft_forward(size), planner.plan_fft_inverse(size));
    let mut a = forward.make_input_vec();
    a[..values.len()].copy_from_slice(values);
    let mut b = forward.make_input_vec();
    for (x, &p) in b.iter_mut().zip(pattern) {
        *x = p as f32;
    }
    let (mut sa, mut sb) = (forward.make_output_vec(), forward.make_output_vec());
    forward.process(&mut a, &mut sa).expect("buffers made by the planner");
    forward.process(&mut b, &mut sb).expect("buffers made by the planner");
    for (x, y) in sa.iter_mut().zip(&sb) {
        *x *= y.conj();
    }
    let mut out = inverse.make_output_vec();
    inverse.process(&mut sa, &mut out).expect("buffers made by the planner");
    out.iter().take(values.len()).map(|&c| f64::from(c) / size as f64).collect()
}

/// How far the kicks struck at `times` are from the beats of `grid`, beyond how far they are on
/// average: the median of each one's distance to its nearest beat, its median taken off.
fn spread(times: &[f64], grid: &BeatGrid) -> f64 {
    let offsets: Vec<f64> = times.iter().map(|&t| t - grid.time_of(grid.beats_at(t).round())).collect();
    let middle = median(offsets.clone());
    median(offsets.iter().map(|o| (o - middle).abs()).collect())
}

/// How far the kicks stray within a run of them (no breakdown between two), where they stray
/// most: the gap between the medians of two of its stretches of beats, the medians of `offsets`
/// from the steady grid. Between two runs, the sound of the kick may change, and the place its
/// likeness peaks with it: the runs are not compared.
fn straying_within_runs(kicks: &[Beat], offsets: &[f64]) -> f64 {
    let mut straying = 0.0f64;
    let mut start = 0;
    for end in 1..=kicks.len() {
        if end < kicks.len() && (kicks[end].beat - kicks[end - 1].beat) as f64 <= GAP_BEATS {
            continue;
        }
        let medians: Vec<f64> = kicks[start..end]
            .iter()
            .zip(&offsets[start..end])
            .collect::<Vec<_>>()
            .chunk_by(|a, b| a.0.beat.div_euclid(STRETCH_BEATS) == b.0.beat.div_euclid(STRETCH_BEATS))
            .filter(|stretch| stretch.len() >= STRETCH_KICKS)
            .map(|stretch| median(stretch.iter().map(|(_, &o)| o).collect()))
            .collect();
        if let (Some(low), Some(high)) =
            (medians.iter().copied().reduce(f64::min), medians.iter().copied().reduce(f64::max))
        {
            straying = straying.max(high - low);
        }
        start = end;
    }
    straying
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

/// The median of each value with its neighbours, `width` in all.
fn running_median(values: &[f64], width: usize) -> Vec<f64> {
    let half = width / 2;
    (0..values.len())
        .map(|i| median(values[i.saturating_sub(half)..(i + half + 1).min(values.len())].to_vec()))
        .collect()
}

/// The least-squares line through the points: its intercept and slope.
fn line_through(xs: &[f64], ys: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    let (mx, my) = (xs.iter().sum::<f64>() / n, ys.iter().sum::<f64>() / n);
    let sxx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
    let sxy: f64 = xs.iter().zip(ys).map(|(x, y)| (x - mx) * (y - my)).sum();
    let slope = if sxx > 0.0 { sxy / sxx } else { 0.0 };
    (my - slope * mx, slope)
}

/// The fewest of the points, the first and the last among them, such that the lines between
/// them pass within `tolerance` of every point (greedy: each line as long as it can go).
fn fewest_anchors(points: &[(f64, f64)], tolerance: f64) -> Vec<(f64, f64)> {
    let mut anchors = vec![points[0]];
    let mut from = 0;
    while from < points.len() - 1 {
        let mut to = from + 1;
        while to + 1 < points.len() && within(&points[from..=to + 1], tolerance) {
            to += 1;
        }
        anchors.push(points[to]);
        from = to;
    }
    anchors
}

/// Whether the line from the first point to the last passes within `tolerance` of them all.
fn within(points: &[(f64, f64)], tolerance: f64) -> bool {
    let ((x0, y0), (x1, y1)) = (points[0], points[points.len() - 1]);
    let slope = (y1 - y0) / (x1 - x0);
    points.iter().all(|&(x, y)| (y0 + slope * (x - x0) - y).abs() <= tolerance)
}
