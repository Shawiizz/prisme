//! The 3-band EQ of a mixer channel, as an isolator: the signal is split into low, mid
//! and high bands by Linkwitz-Riley crossovers (24 dB per octave) and the bands are
//! summed back with their gains. At unity the bands add up to the original magnitude
//! (the low band goes through the high crossover's all-pass so that its phase matches);
//! a band at zero is gone (kill).

use crate::smooth::Smoothed;
use crate::svf::{Coefficients, Svf, BUTTERWORTH_Q};

pub const LOW_MID_HZ: f32 = 250.0;
pub const MID_HIGH_HZ: f32 = 3_000.0;
/// Gains glide over this time.
const GLIDE_S: f32 = 0.005;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Low,
    Mid,
    High,
}

/// A Linkwitz-Riley crossover: low = two Butterworth low-passes, high = two Butterworth
/// high-passes (the first stage gives both).
#[derive(Debug, Clone, Copy, Default)]
struct Crossover {
    first: Svf,
    low: Svf,
    high: Svf,
}

impl Crossover {
    #[inline]
    fn split(&mut self, c: &Coefficients, x: f32) -> (f32, f32) {
        let first = self.first.process(c, x);
        (self.low.process(c, first.low).low, self.high.process(c, first.high).high)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Channel {
    low_mid: Crossover,
    mid_high: Crossover,
    /// The all-pass of the mid/high crossover, for the low band.
    low_phase: Crossover,
}

impl Channel {
    #[inline]
    fn process(&mut self, c: &[Coefficients; 2], gains: [f32; 3], x: f32) -> f32 {
        let (low, rest) = self.low_mid.split(&c[0], x);
        let (mid, high) = self.mid_high.split(&c[1], rest);
        let (a, b) = self.low_phase.split(&c[1], low);
        gains[0] * (a + b) + gains[1] * mid + gains[2] * high
    }
}

pub struct IsolatorEq {
    coefficients: [Coefficients; 2],
    gains: [Smoothed; 3],
    channels: [Channel; 2],
}

impl IsolatorEq {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            coefficients: [
                Coefficients::new(LOW_MID_HZ, BUTTERWORTH_Q, sample_rate),
                Coefficients::new(MID_HIGH_HZ, BUTTERWORTH_Q, sample_rate),
            ],
            gains: [Smoothed::new(1.0, GLIDE_S, sample_rate); 3],
            channels: [Channel::default(); 2],
        }
    }

    /// Linear gain of a band: 1 leaves it as it is, 0 removes it.
    pub fn set_gain(&mut self, band: Band, gain: f32) {
        self.gains[band as usize].set(gain.max(0.0));
    }

    pub fn gain(&self, band: Band) -> f32 {
        self.gains[band as usize].target()
    }

    /// Processes interleaved stereo in place.
    pub fn process(&mut self, stereo: &mut [f32]) {
        for frame in stereo.chunks_exact_mut(2) {
            let gains = [self.gains[0].tick(), self.gains[1].tick(), self.gains[2].tick()];
            for (sample, channel) in frame.iter_mut().zip(&mut self.channels) {
                *sample = channel.process(&self.coefficients, gains, *sample);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::response_db;

    fn response(freq: f64, gains: [f32; 3]) -> f64 {
        let mut eq = IsolatorEq::new(48_000.0);
        for (band, gain) in [Band::Low, Band::Mid, Band::High].into_iter().zip(gains) {
            eq.set_gain(band, gain);
        }
        response_db(freq, 48_000.0, |block| eq.process(block))
    }

    #[test]
    fn flat_at_unity() {
        for freq in [40.0, 250.0, 700.0, 3_000.0, 12_000.0] {
            let db = response(freq, [1.0, 1.0, 1.0]);
            assert!(db.abs() < 0.1, "{freq} Hz: {db:.2} dB");
        }
    }

    #[test]
    fn kills_remove_their_band_only() {
        assert!(response(50.0, [0.0, 1.0, 1.0]) < -40.0, "bass killed");
        assert!(response(1_000.0, [0.0, 1.0, 1.0]).abs() < 0.5, "mids kept");
        assert!(response(1_000.0, [1.0, 0.0, 1.0]) < -20.0, "mids killed");
        assert!(response(12_000.0, [1.0, 1.0, 0.0]) < -40.0, "highs killed");
        assert!(response(50.0, [1.0, 1.0, 0.0]).abs() < 0.5, "bass kept");
    }

    #[test]
    fn boost() {
        let db = response(50.0, [2.0, 1.0, 1.0]);
        assert!((db - 6.0).abs() < 0.3, "{db:.2} dB");
    }
}
