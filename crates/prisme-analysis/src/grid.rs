//! Beat grid on the kicks' attacks, to the sample.
//!
//! The tempo found on the onset envelope ([`crate::tempo`]) places the beats to within a
//! few tens of milliseconds. Around each beat, the envelope of the signal is taken; the
//! beats where the kick band clearly rises hold a kick (the beats of a breakdown do not).
//! Averaged, the kicks' envelopes picture the track's kick without the rest of the music;
//! on that picture, the attack is where the kick starts, where a DJ puts the beat on the
//! waveform (not its loudest instant, later and different for every kick). Each kick is
//! then aligned on the picture, which gives the instant its own attack starts. The tempo is
//! the median period between kicks a few bars apart (a whole BPM when it fits as well),
//! and the grid goes on the median of the attacks.

use crate::dsp::{dot, MonoSignal};
use crate::tempo::Tempo;
use realfft::{RealFftPlanner, RealToComplex};
use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use std::f64::consts::PI;
use std::ops::Range;
use std::sync::Arc;

/// The envelope kept around each beat (seconds before, after).
const AROUND_S: (f64, f64) = (0.07, 0.08);
/// How far from the beats of a grid the kicks are looked for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Reach {
    /// Where the kick's body is (seconds before, after a beat).
    body_s: (f64, f64),
    /// How far a kick may be from the others (seconds), once aligned on their picture.
    lag_s: f64,
}

/// Around a beat of the tempo's grid, wide: an onset envelope frame is 46 ms long, and the
/// attack of a kick may lie anywhere in its first half; over a long track, the tempo's
/// small error adds up. Around an attack, narrow: the body follows it, a second rise
/// further on (a layer, a bass note) is not the kick, and the kicks of a track made in a
/// DAW are where the grid says.
const COARSE: Reach = Reach { body_s: (0.025, 0.045), lag_s: 0.012 };
const ON_KICKS: Reach = Reach { body_s: (0.01, 0.035), lag_s: 0.004 };
/// Passes on a grid already on the attacks, at most: each pictures the kick around the
/// attacks found, until the grid moves less than this.
const MAX_PASSES: usize = 3;
const SETTLED_S: f64 = 0.000_5;
/// The kick band: the body of a kick, below the bass notes' upper harmonics and the snare.
/// Band edges are smooth, so as not to ring before an attack.
const KICK_BAND_HZ: (f64, f64) = (30.0, 180.0);
/// The wide band, where the click that starts a kick is. It has no low edge: a steep one
/// rings long, before an attack as after it.
const WIDE_BAND_HZ: (f64, f64) = (0.0, 3_000.0);
/// Growth of the kick's body is measured over this span.
const BODY_RISE_S: f64 = 0.01;
/// The attack is looked for up to this long before the body grows most, in a stretch that
/// goes on this long after it.
const ATTACK_LEAD_S: f64 = 0.03;
const ATTACK_TAIL_S: f64 = 0.015;
/// Power, relative to the kick's, that the attack must rise out of.
const NOISE_FLOOR: f64 = 0.05;
/// A beat whose kick band grows less than this share of the strong kicks' holds no kick.
const MIN_STRENGTH: f32 = 0.3;
/// A kick is aligned on the picture of the track's kick over the stretch of the picture from
/// a little before its attack to a little after.
const ALIGN_S: (f64, f64) = (0.015, 0.03);
/// Fewer kicks than this and the grid is not fitted on them.
const MIN_KICKS: usize = 16;
/// The beat period is measured between kicks this many beats apart: far enough for
/// precision, close enough that most pairs lie within a section of the track, so that a
/// section whose kick sounds a little different does not tilt the tempo.
const PAIR_BEATS: usize = 32;
/// A whole BPM (DAW-made tracks) is kept if, in every stretch of this many kicks, it keeps
/// the grid within this much of the kicks, beyond what the measured tempo does.
const SNAP_STRETCH: usize = 32;
const SNAP_TOLERANCE_S: f64 = 0.002;

/// The attack of a kick near a beat of a grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Attack {
    /// Beat number on the grid it was looked for on (0: its first beat).
    pub beat: usize,
    /// Where the attack starts (seconds).
    pub time_s: f64,
}

/// The tempo `coarse` with its BPM and phase measured on the attacks of the kicks; `coarse`
/// itself if the track has too few kicks.
pub fn fit_grid(signal: &MonoSignal, coarse: Tempo) -> Tempo {
    let (mut period, mut phase) = (coarse.beat_period_s(), coarse.first_beat_s);
    let mut attacks = attacks_near(signal, period, phase, COARSE);
    // passes on a grid on the kicks also picture the kick without the blur that the
    // tempo's small error makes over a long track
    for _ in 0..MAX_PASSES {
        if attacks.len() < MIN_KICKS {
            return coarse;
        }
        let previous = phase;
        period = beat_period(&attacks).unwrap_or(period);
        phase = grid_phase(&attacks, period);
        let moved = (phase - previous).rem_euclid(period);
        if moved.min(period - moved) < SETTLED_S {
            break;
        }
        attacks = attacks_near(signal, period, phase, ON_KICKS);
    }
    let whole = 60.0 / (60.0 / period).round();
    if spread(&attacks, whole) <= spread(&attacks, period) + SNAP_TOLERANCE_S {
        (period, phase) = (whole, grid_phase(&attacks, whole));
    }
    Tempo { bpm: 60.0 / period, first_beat_s: phase.rem_euclid(period), ..coarse }
}

/// The attacks of the kicks near the beats of a grid that is on the kicks, even those up to
/// 12 ms from where the others put them (beats without a kick are missing): to measure how
/// well a grid fits.
pub fn kick_attacks(signal: &MonoSignal, bpm: f64, first_beat_s: f64) -> Vec<Attack> {
    attacks_near(signal, 60.0 / bpm, first_beat_s, Reach { lag_s: COARSE.lag_s, ..ON_KICKS })
}

/// The attacks of the kicks near the beats of a grid.
fn attacks_near(signal: &MonoSignal, period: f64, first_beat_s: f64, reach: Reach) -> Vec<Attack> {
    let first = first_beat_s.rem_euclid(period);
    let beats = ((signal.duration_s() - first) / period).ceil().max(0.0) as usize;
    let mut windows = BeatWindows::new(signal.sample_rate);
    let mut kicks: Vec<Kick> = (0..beats)
        .map(|beat| windows.kick(signal, beat, first + beat as f64 * period, reach.body_s))
        .collect();
    let mut strengths: Vec<f32> = kicks.iter().map(|k| k.strength).collect();
    strengths.sort_by(f32::total_cmp);
    let Some(&strong) = strengths.get(strengths.len() * 3 / 4) else { return Vec::new() };
    kicks.retain(|k| k.strength > 0.0 && k.strength >= MIN_STRENGTH * strong);
    if kicks.is_empty() {
        return Vec::new();
    }

    let rate = signal.sample_rate;
    let samples = |s: f64| (s * rate).round() as usize;
    let zero = samples(AROUND_S.0);
    let max_lag = samples(reach.lag_s);
    let mut lags = vec![0.0; kicks.len()];
    for _ in 0..2 {
        let picture = average(&kicks, &lags);
        let onset = attack_of(&picture, &power(signal, &kicks, &lags), rate, reach.body_s);
        let stretch = onset.saturating_sub(samples(ALIGN_S.0))..onset + samples(ALIGN_S.1);
        for (lag, kick) in lags.iter_mut().zip(&kicks) {
            *lag = alignment(&picture, &kick.envelope, stretch.clone(), max_lag);
        }
    }
    let onset = attack_of(&average(&kicks, &lags), &power(signal, &kicks, &lags), rate, reach.body_s) as f64;
    kicks
        .iter()
        .zip(&lags)
        .filter(|(_, lag)| lag.abs() < max_lag as f64)
        .map(|(k, lag)| Attack { beat: k.beat, time_s: k.at_s + (onset + lag - zero as f64) / rate })
        .collect()
}

/// The envelope of the wide band around a beat, and how much its kick band grows there.
struct Kick {
    beat: usize,
    at_s: f64,
    envelope: Vec<f32>,
    strength: f32,
}

/// The average of the kicks' envelopes, each shifted by its lag (whole samples) and
/// weighted equally whatever its loudness.
fn average(kicks: &[Kick], lags: &[f64]) -> Vec<f32> {
    let len = kicks[0].envelope.len();
    let mut sum = vec![0.0f64; len];
    let mut count = vec![0usize; len];
    for (kick, &lag) in kicks.iter().zip(lags) {
        let level = kick.envelope.iter().map(|&e| f64::from(e)).sum::<f64>() / len as f64;
        if level <= 0.0 {
            continue;
        }
        let shift = lag.round() as isize;
        for (i, (s, c)) in sum.iter_mut().zip(&mut count).enumerate() {
            if let Some(&e) = kick.envelope.get((i as isize + shift) as usize) {
                *s += f64::from(e) / level;
                *c += 1;
            }
        }
    }
    sum.iter().zip(&count).map(|(&s, &c)| if c > 0 { (s / c as f64) as f32 } else { 0.0 }).collect()
}

/// The average power of the signal around the kicks, each shifted by its lag (whole
/// samples) and weighted equally whatever its loudness. Unlike an envelope, it does not
/// rise before the kick starts.
fn power(signal: &MonoSignal, kicks: &[Kick], lags: &[f64]) -> Vec<f64> {
    let rate = signal.sample_rate;
    let len = kicks[0].envelope.len();
    let before = (AROUND_S.0 * rate).round() as isize;
    let mut sum = vec![0.0f64; len];
    let mut count = vec![0usize; len];
    for (kick, &lag) in kicks.iter().zip(lags) {
        let start = (kick.at_s * rate).round() as isize - before + lag.round() as isize;
        let at = |i: usize| usize::try_from(start + i as isize).ok().and_then(|j| signal.samples.get(j));
        let level = (0..len).filter_map(at).map(|&x| f64::from(x) * f64::from(x)).sum::<f64>() / len as f64;
        if level <= 0.0 {
            continue;
        }
        for (i, (s, c)) in sum.iter_mut().zip(&mut count).enumerate() {
            if let Some(&x) = at(i) {
                *s += f64::from(x) * f64::from(x) / level;
                *c += 1;
            }
        }
    }
    sum.iter().zip(&count).map(|(&s, &c)| if c > 0 { s / c as f64 } else { 0.0 }).collect()
}

/// The lag (samples, fractional) at which `envelope` best matches `picture` over `stretch`:
/// the peak of the correlation of their slopes, so that rises meet rises. A stretch past the
/// picture's end (an attack found at its very end) stops there.
fn alignment(picture: &[f32], envelope: &[f32], stretch: Range<usize>, max_lag: usize) -> f64 {
    let end = stretch.end.min(picture.len());
    let stretch = stretch.start.min(end)..end;
    if stretch.len() < 2 {
        return 0.0;
    }
    let slopes = |e: &[f32]| -> Vec<f32> { e.windows(2).map(|w| w[1] - w[0]).collect() };
    let (target, slope) = (slopes(&picture[stretch.clone()]), slopes(envelope));
    let scores: Vec<f32> = (0..=2 * max_lag)
        .map(|l| {
            let from = (stretch.start + l).checked_sub(max_lag);
            from.and_then(|f| slope.get(f..f + target.len())).map_or(f32::NEG_INFINITY, |s| dot(s, &target))
        })
        .collect();
    let best = (0..scores.len()).max_by(|&a, &b| scores[a].total_cmp(&scores[b])).unwrap_or(max_lag);
    let delta = match (best.checked_sub(1).map(|i| scores[i]), scores.get(best + 1)) {
        (Some(l), Some(&r)) if l.is_finite() && r.is_finite() => {
            let denom = l - 2.0 * scores[best] + r;
            if denom < 0.0 {
                f64::from(0.5 * (l - r) / denom).clamp(-0.5, 0.5)
            } else {
                0.0
            }
        }
        _ => 0.0,
    };
    best as f64 - max_lag as f64 + delta
}

/// Where the kick starts (sample), from the average envelope and power around the kicks:
/// the change of power that best splits the stretch before the body into calm then loud
/// (by the Akaike information criterion), the body being where the envelope grows most.
fn attack_of(envelope: &[f32], power: &[f64], rate: f64, body_s: (f64, f64)) -> usize {
    let samples = |s: f64| (s * rate).round() as usize;
    let zero = samples(AROUND_S.0);
    let span = samples(BODY_RISE_S);
    let search = zero - samples(body_s.0)..zero + samples(body_s.1);
    let (body, _) = steepest(envelope, search, span);
    let range = body.saturating_sub(samples(ATTACK_LEAD_S)).max(1)
        ..(body + samples(ATTACK_TAIL_S)).min(envelope.len());
    let power = &power[range.clone()];
    let n = power.len();
    let mut prefix = vec![0.0; n + 1];
    for (i, p) in power.iter().enumerate() {
        prefix[i + 1] = prefix[i] + p;
    }
    // a floor far below the kick, as a little noise would be: in silence, the faint ringing
    // of the filters ahead of a kick would otherwise count as its start
    let floor = NOISE_FLOOR * prefix[n] / n as f64;
    let aic = |k: usize| {
        let before = prefix[k] / k as f64 + floor;
        let after = (prefix[n] - prefix[k]) / (n - k) as f64 + floor;
        k as f64 * before.ln() + (n - k) as f64 * after.ln()
    };
    range.start + (2..n.saturating_sub(1)).min_by(|&a, &b| aic(a).total_cmp(&aic(b))).unwrap_or(0)
}

/// Where in `range` `envelope` grows most, and that growth: the mean of the `span` samples
/// after minus the mean of the `span` before.
fn steepest(envelope: &[f32], range: Range<usize>, span: usize) -> (usize, f32) {
    let mut before: f32 = envelope[range.start - span..range.start].iter().sum();
    let mut after: f32 = envelope[range.start..range.start + span].iter().sum();
    let mut best = (range.start, f32::NEG_INFINITY);
    for i in range {
        if after - before > best.1 {
            best = (i, after - before);
        }
        before += envelope[i] - envelope[i - span];
        after += envelope[i + span] - envelope[i];
    }
    (best.0, best.1 / span as f32)
}

/// The median period between kicks [`PAIR_BEATS`] apart.
fn beat_period(attacks: &[Attack]) -> Option<f64> {
    let span = attacks.last()?.beat - attacks.first()?.beat;
    let gap = PAIR_BEATS.min(span / 2).max(1);
    let mut j = 0;
    let periods: Vec<f64> = attacks
        .iter()
        .filter_map(|a| {
            while attacks.get(j)?.beat < a.beat + gap {
                j += 1;
            }
            let b = attacks[j];
            Some((b.time_s - a.time_s) / (b.beat - a.beat) as f64)
        })
        .collect();
    (!periods.is_empty()).then(|| median(periods))
}

/// The time of beat 0 of the grid at `period` closest to most kicks.
fn grid_phase(attacks: &[Attack], period: f64) -> f64 {
    median(attacks.iter().map(|a| a.time_s - a.beat as f64 * period).collect())
}

/// How far the grid at `period` strays from the kicks where it strays most: the largest
/// distance between the grid and the median attack of a stretch of [`SNAP_STRETCH`] kicks.
fn spread(attacks: &[Attack], period: f64) -> f64 {
    let phase = grid_phase(attacks, period);
    attacks
        .chunks(SNAP_STRETCH)
        .map(|stretch| (grid_phase(stretch, period) - phase).abs())
        .fold(0.0, f64::max)
}

fn median(mut values: Vec<f64>) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mid = values.len() / 2;
    *values.select_nth_unstable_by(mid, f64::total_cmp).1
}

/// Envelopes of the kick band and of the wide band around the beats, from the analytic
/// signal of each band over a window.
struct BeatWindows {
    rate: f64,
    size: usize,
    taper: Vec<f32>,
    forward: Arc<dyn RealToComplex<f32>>,
    inverse: Arc<dyn Fft<f32>>,
    frame: Vec<f32>,
    spectrum: Vec<Complex32>,
    analytic: Vec<Complex32>,
    scratch: Vec<Complex32>,
    kick: Band,
    wide: Band,
}

/// A band's filter, with the analytic-signal weighting, per bin, and its envelope.
struct Band {
    gains: Vec<f32>,
    envelope: Vec<f32>,
}

impl Band {
    fn new(size: usize, rate: f64, (low_hz, high_hz): (f64, f64)) -> Self {
        let edge = |x: f64| 0.5 - 0.5 * (PI * x.clamp(0.0, 1.0)).cos();
        let gains = (0..size / 2)
            .map(|k| {
                let hz = k as f64 * rate / size as f64;
                let low = if low_hz > 0.0 { edge(2.0 * hz / low_hz - 1.0) } else { 1.0 };
                let pass = if k == 0 { 0.0 } else { low * (1.0 - edge((hz - high_hz) / (0.8 * high_hz))) };
                // twice the positive frequencies, none of the negative ones: the analytic
                // signal, whose magnitude is the envelope
                (2.0 * pass / size as f64) as f32
            })
            .collect();
        Self { gains, envelope: vec![0.0; size] }
    }
}

impl BeatWindows {
    fn new(rate: f64) -> Self {
        // twice the envelope kept: the taper stays outside it, and what the band filters
        // smear past one end of the window does not wrap round onto it
        let size = (((AROUND_S.0 + AROUND_S.1) * 2.0 * rate).ceil() as usize).next_power_of_two();
        let forward = RealFftPlanner::new().plan_fft_forward(size);
        let inverse = FftPlanner::new().plan_fft_inverse(size);
        let taper = (0..size)
            .map(|i| {
                let d = (i.min(size - 1 - i) as f64 / (size as f64 / 4.0)).min(1.0);
                (0.5 - 0.5 * (PI * d).cos()) as f32
            })
            .collect();
        Self {
            rate,
            size,
            taper,
            frame: forward.make_input_vec(),
            spectrum: forward.make_output_vec(),
            analytic: vec![Complex32::default(); size],
            scratch: vec![
                Complex32::default();
                inverse.get_inplace_scratch_len().max(forward.get_scratch_len())
            ],
            kick: Band::new(size, rate, KICK_BAND_HZ),
            wide: Band::new(size, rate, WIDE_BAND_HZ),
            forward,
            inverse,
        }
    }

    fn kick(&mut self, signal: &MonoSignal, beat: usize, at_s: f64, body: (f64, f64)) -> Kick {
        let start = (at_s * self.rate).round() as isize - (self.size / 2) as isize;
        self.envelopes(&signal.samples, start);
        let samples = |s: f64| (s * self.rate).round() as usize;
        let mid = self.size / 2;
        let span = samples(BODY_RISE_S);
        let (_, strength) = steepest(&self.kick.envelope, mid - samples(body.0)..mid + samples(body.1), span);
        let around = mid - samples(AROUND_S.0)..mid + samples(AROUND_S.1);
        Kick {
            beat,
            at_s: (start + mid as isize) as f64 / self.rate,
            envelope: self.wide.envelope[around].to_vec(),
            strength,
        }
    }

    /// Envelopes of the bands over the window starting at sample `start` (silence outside
    /// the signal).
    fn envelopes(&mut self, samples: &[f32], start: isize) {
        for (i, (f, w)) in self.frame.iter_mut().zip(&self.taper).enumerate() {
            let s = start + i as isize;
            *f = if s >= 0 { samples.get(s as usize).map_or(0.0, |x| x * w) } else { 0.0 };
        }
        self.forward
            .process_with_scratch(&mut self.frame, &mut self.spectrum, &mut self.scratch)
            .expect("buffers made by the planner");
        for band in [&mut self.kick, &mut self.wide] {
            self.analytic.fill(Complex32::default());
            for ((a, s), g) in self.analytic.iter_mut().zip(&self.spectrum).zip(&band.gains) {
                *a = s * *g;
            }
            self.inverse.process_with_scratch(&mut self.analytic, &mut self.scratch);
            for (e, a) in band.envelope.iter_mut().zip(&self.analytic) {
                *e = a.norm();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stretch_past_the_picture_stops_at_its_end() {
        let picture: Vec<f32> = (0..100).map(|i| if i < 90 { 0.0 } else { 1.0 }).collect();
        let lag = alignment(&picture, &picture, 80..130, 5);
        assert!(lag.abs() < 0.5, "{lag}");
        assert_eq!(alignment(&picture, &picture, 120..130, 5), 0.0, "nothing left to match");
    }
}
