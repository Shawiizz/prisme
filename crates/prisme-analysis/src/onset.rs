//! Onset strength envelopes: how much new energy appears at each instant, from the
//! log-compressed spectral flux.
//!
//! Two envelopes come out of the same transform:
//! * `full`: every band, rich in periodicities (kick, snare, hats): used to find the
//!   tempo;
//! * `low`: the bass band only, i.e. mostly the kick: used to put the beat grid on
//!   the kicks rather than on off-beat hats.

use crate::dsp::{MonoSignal, Stft};

/// 46 ms frames every 5.8 ms (at 11 025 Hz).
const FRAME: usize = 512;
const HOP: usize = 64;
/// Upper edge of the "low" band.
const LOW_BAND_HZ: f64 = 180.0;
/// Log compression: `ln(1 + COMPRESSION * magnitude)`.
const COMPRESSION: f32 = 1_000.0;
/// Width of the moving average removed from the envelopes (seconds).
const DETREND_S: f64 = 0.4;
/// A sharp attack peaks in the envelope while it is still ahead of the frame centre,
/// by this much (seconds, measured on synthetic kicks at many tempos).
const ATTACK_AHEAD_OF_CENTRE_S: f64 = 0.0153;

pub struct OnsetEnvelopes {
    pub full: Vec<f32>,
    pub low: Vec<f32>,
    /// Envelope values per second.
    pub frame_rate: f64,
    /// Time of the attacks that peak at value 0 (seconds).
    pub start_s: f64,
}

impl OnsetEnvelopes {
    pub fn len(&self) -> usize {
        self.full.len()
    }

    pub fn is_empty(&self) -> bool {
        self.full.is_empty()
    }

    /// Time of an attack that peaks at envelope value `i` (fractional indices allowed).
    pub fn time_of(&self, i: f64) -> f64 {
        self.start_s + i / self.frame_rate
    }
}

pub fn onset_envelopes(signal: &MonoSignal) -> OnsetEnvelopes {
    let mut stft = Stft::new(FRAME, HOP);
    let frames = stft.frame_count(signal.samples.len());
    let low_bins = ((LOW_BAND_HZ * FRAME as f64 / signal.sample_rate).ceil() as usize).max(2);

    let mut full = Vec::with_capacity(frames);
    let mut low = Vec::with_capacity(frames);
    let mut previous = vec![0.0f32; FRAME / 2 + 1];
    stft.for_each_frame(&signal.samples, |frame, magnitudes| {
        let (mut f, mut l) = (0.0f32, 0.0f32);
        for (k, (m, p)) in magnitudes.iter().zip(previous.iter_mut()).enumerate() {
            let v = (1.0 + COMPRESSION * m).ln();
            let rise = (v - *p).max(0.0);
            // bin 0 (DC) carries no onset information
            if k > 0 {
                f += rise;
                if k <= low_bins {
                    l += rise;
                }
            }
            *p = v;
        }
        // the first frame has nothing to compare with
        if frame == 0 {
            f = 0.0;
            l = 0.0;
        }
        full.push(f);
        low.push(l);
    });

    let frame_rate = signal.sample_rate / HOP as f64;
    let detrend = (DETREND_S * frame_rate).round() as usize;
    OnsetEnvelopes {
        full: detrended(&full, detrend),
        low: detrended(&low, detrend),
        frame_rate,
        start_s: (FRAME / 2) as f64 / signal.sample_rate + ATTACK_AHEAD_OF_CENTRE_S,
    }
}

/// Subtracts a centred moving average of `width` values and keeps the positive part:
/// leaves the sharp peaks, removes the slow loudness changes.
fn detrended(values: &[f32], width: usize) -> Vec<f32> {
    let half = width / 2;
    let mut prefix = Vec::with_capacity(values.len() + 1);
    prefix.push(0.0f64);
    for &v in values {
        prefix.push(prefix.last().unwrap() + v as f64);
    }
    (0..values.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(half), (i + half + 1).min(values.len()));
            let mean = (prefix[b] - prefix[a]) / (b - a) as f64;
            (values[i] - mean as f32).max(0.0)
        })
        .collect()
}
