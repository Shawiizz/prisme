//! Tempo and beat grid of a constant-tempo track (the norm in electronic music).
//!
//! 1. The Fourier spectrum of the whole onset envelope shows one sharp line per
//!    periodicity of the track. A beat at `b` BPM puts lines at `b`, `2b`, `3b`...
//!    Each candidate of the BPM range (one octave, so every pulse has exactly one
//!    representative) is scored by summing its first harmonics: this prefers the
//!    real beat over the 3:2 relations that a single line would allow.
//! 2. The winner is refined to about 0.001 BPM by maximising the same harmonic sum
//!    with exact DFTs (precision grows with the track length), then snapped to the
//!    nearest whole BPM when that fits as well (DAW-made tracks).
//! 3. The grid phase is the offset whose beats collect the most bass onsets, so the
//!    grid sits on the kicks.

use crate::onset::OnsetEnvelopes;
use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;
use std::f64::consts::TAU;

/// Shorter tracks are not analysed.
const MIN_DURATION_S: f64 = 10.0;
/// Harmonics summed by the coarse scan and by the refinement.
const COARSE_HARMONICS: usize = 8;
const FINE_HARMONICS: usize = 4;
/// Coarse scan step and refinement search half-width (BPM).
const COARSE_STEP_BPM: f64 = 0.01;
const FINE_SPAN_BPM: f64 = 0.06;
/// Phase search step (envelope frames).
const PHASE_STEP: f64 = 0.25;
/// Tracks made in a DAW have a whole BPM: a refined value this close to an integer
/// is snapped to it if the integer explains the onsets almost as well.
const SNAP_DISTANCE_BPM: f64 = 0.03;
const SNAP_MIN_SCORE_RATIO: f64 = 0.98;

/// Default lower bound of the BPM range: [88, 176) holds house, techno, schranz and
/// drum and bass at their usual tempo.
pub const DEFAULT_MIN_BPM: f64 = 88.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tempo {
    pub bpm: f64,
    /// Time of the first beat at or after the start of the track (seconds).
    pub first_beat_s: f64,
    /// 0 (no clear pulse) to 1 (a single dominant pulse).
    pub confidence: f32,
}

impl Tempo {
    pub fn beat_period_s(&self) -> f64 {
        60.0 / self.bpm
    }
}

/// Estimates the tempo in `[min_bpm, 2 * min_bpm)`. `None` for short or beatless audio.
pub fn estimate_tempo(env: &OnsetEnvelopes, min_bpm: f64) -> Option<Tempo> {
    if (env.len() as f64) / env.frame_rate < MIN_DURATION_S {
        return None;
    }
    let signal = zero_mean(&env.full);
    if signal.iter().all(|&v| v.abs() < 1e-9) {
        return None;
    }

    let spectrum = Spectrum::of(&signal, env.frame_rate);
    let steps = (min_bpm / COARSE_STEP_BPM).round() as usize;
    let (mut best_bpm, mut best_score, mut total) = (min_bpm, 0.0f64, 0.0f64);
    for i in 0..steps {
        let bpm = min_bpm + i as f64 * COARSE_STEP_BPM;
        let score: f64 = (1..=COARSE_HARMONICS)
            .map(|k| spectrum.max_near(bpm * k as f64, COARSE_STEP_BPM * k as f64 / 2.0))
            .sum();
        total += score;
        if score > best_score {
            (best_bpm, best_score) = (bpm, score);
        }
    }
    if best_score <= 0.0 {
        return None;
    }
    let confidence = (1.0 - total / steps as f64 / best_score).clamp(0.0, 1.0) as f32;

    let refined = refine(&signal, env.frame_rate, best_bpm);
    let whole = refined.round();
    let bpm = if (refined - whole).abs() <= SNAP_DISTANCE_BPM
        && harmonic_score(&signal, env.frame_rate, whole)
            >= SNAP_MIN_SCORE_RATIO * harmonic_score(&signal, env.frame_rate, refined)
    {
        whole
    } else {
        refined
    };
    let first_beat_s = beat_phase(env, bpm);
    Some(Tempo { bpm, first_beat_s, confidence })
}

fn zero_mean(values: &[f32]) -> Vec<f64> {
    let mean = values.iter().map(|&v| v as f64).sum::<f64>() / values.len().max(1) as f64;
    values.iter().map(|&v| v as f64 - mean).collect()
}

/// Magnitude spectrum of the envelope, zero-padded for a fine frequency grid.
struct Spectrum {
    magnitudes: Vec<f32>,
    /// BPM per bin.
    bin_bpm: f64,
}

impl Spectrum {
    fn of(signal: &[f64], frame_rate: f64) -> Self {
        let size = (signal.len() * 4).next_power_of_two().max(1 << 16);
        let mut buffer: Vec<Complex32> = signal.iter().map(|&v| Complex32::new(v as f32, 0.0)).collect();
        buffer.resize(size, Complex32::default());
        FftPlanner::new().plan_fft_forward(size).process(&mut buffer);
        Self {
            magnitudes: buffer[..size / 2].iter().map(|c| c.norm()).collect(),
            bin_bpm: frame_rate * 60.0 / size as f64,
        }
    }

    /// Largest magnitude within `half_width` BPM of `bpm` (at least the nearest bin).
    fn max_near(&self, bpm: f64, half_width: f64) -> f64 {
        let lo = ((bpm - half_width) / self.bin_bpm).floor().max(0.0) as usize;
        let hi = ((bpm + half_width) / self.bin_bpm).ceil() as usize;
        let hi = hi.min(self.magnitudes.len() - 1);
        self.magnitudes.get(lo..=hi).map_or(0.0, |m| m.iter().cloned().fold(0.0, f32::max) as f64)
    }
}

/// Magnitude of the DFT of `signal` at `hz`.
fn dft_magnitude(signal: &[f64], frame_rate: f64, hz: f64) -> f64 {
    let w = TAU * hz / frame_rate;
    let (step_re, step_im) = (w.cos(), -w.sin());
    let (mut z_re, mut z_im) = (1.0f64, 0.0f64);
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (n, &x) in signal.iter().enumerate() {
        re += x * z_re;
        im += x * z_im;
        (z_re, z_im) = (z_re * step_re - z_im * step_im, z_re * step_im + z_im * step_re);
        // keep the rotating phasor on the unit circle
        if n % 4096 == 4095 {
            let norm = (z_re * z_re + z_im * z_im).sqrt();
            (z_re, z_im) = (z_re / norm, z_im / norm);
        }
    }
    (re * re + im * im).sqrt()
}

/// How well a pulse at `bpm` explains the onsets: its first harmonics, exactly.
fn harmonic_score(signal: &[f64], frame_rate: f64, bpm: f64) -> f64 {
    (1..=FINE_HARMONICS).map(|k| dft_magnitude(signal, frame_rate, bpm * k as f64 / 60.0)).sum()
}

/// Golden-section search of the harmonic score around `bpm`.
fn refine(signal: &[f64], frame_rate: f64, bpm: f64) -> f64 {
    let score = |b: f64| harmonic_score(signal, frame_rate, b);
    let ratio = (5f64.sqrt() - 1.0) / 2.0;
    let (mut a, mut b) = (bpm - FINE_SPAN_BPM, bpm + FINE_SPAN_BPM);
    let mut c = b - ratio * (b - a);
    let mut d = a + ratio * (b - a);
    let (mut fc, mut fd) = (score(c), score(d));
    while b - a > 1e-4 {
        if fc > fd {
            (b, d, fd) = (d, c, fc);
            c = b - ratio * (b - a);
            fc = score(c);
        } else {
            (a, c, fc) = (c, d, fd);
            d = a + ratio * (b - a);
            fd = score(d);
        }
    }
    (a + b) / 2.0
}

/// Offset of the beat grid (seconds, in `[0, period)`): the phase whose beats land on
/// the most bass onsets (all onsets if the track has no bass).
fn beat_phase(env: &OnsetEnvelopes, bpm: f64) -> f64 {
    let low_energy: f64 = env.low.iter().map(|&v| v as f64).sum();
    let full_energy: f64 = env.full.iter().map(|&v| v as f64).sum();
    let values = if low_energy > 0.02 * full_energy { &env.low } else { &env.full };

    let period = 60.0 / bpm * env.frame_rate;
    let sample = |pos: f64| -> f64 {
        let i = pos.floor() as usize;
        let t = pos - i as f64;
        match (values.get(i), values.get(i + 1)) {
            (Some(&a), Some(&b)) => a as f64 * (1.0 - t) + b as f64 * t,
            (Some(&a), None) => a as f64,
            _ => 0.0,
        }
    };
    let score = |phase: f64| -> f64 {
        let mut s = 0.0;
        let mut pos = phase;
        while pos < values.len() as f64 {
            s += sample(pos);
            pos += period;
        }
        s
    };

    let steps = (period / PHASE_STEP).ceil() as usize;
    let scores: Vec<f64> = (0..steps).map(|i| score(i as f64 * PHASE_STEP)).collect();
    let best = (0..steps).max_by(|&a, &b| scores[a].total_cmp(&scores[b])).unwrap_or(0);
    // parabolic interpolation between the neighbouring steps (circular)
    let (l, c, r) = (scores[(best + steps - 1) % steps], scores[best], scores[(best + 1) % steps]);
    let denom = l - 2.0 * c + r;
    let delta = if denom.abs() > 1e-12 { (0.5 * (l - r) / denom).clamp(-0.5, 0.5) } else { 0.0 };
    let phase_frames = (best as f64 + delta) * PHASE_STEP;

    let period_s = 60.0 / bpm;
    env.time_of(phase_frames).rem_euclid(period_s)
}
