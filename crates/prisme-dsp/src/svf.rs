//! State-variable filter in its "topology-preserving transform" form (Zavalishin, "The
//! Art of VA Filter Design"): stable and smooth even when its cutoff moves at every
//! sample, which DJ filter sweeps do. One filter computes the low-pass, band-pass and
//! high-pass outputs at once.

use std::f32::consts::PI;

/// Q of a Butterworth 2-pole stage.
pub const BUTTERWORTH_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

#[derive(Debug, Clone, Copy, Default)]
pub struct Coefficients {
    g: f32,
    k: f32,
    a1: f32,
}

impl Coefficients {
    pub fn new(cutoff_hz: f32, q: f32, sample_rate: f32) -> Self {
        let cutoff = cutoff_hz.clamp(10.0, sample_rate * 0.49);
        let g = (PI * cutoff / sample_rate).tan();
        let k = 1.0 / q;
        Self { g, k, a1: 1.0 / (1.0 + g * (g + k)) }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Outputs {
    pub low: f32,
    pub band: f32,
    pub high: f32,
}

/// One channel of a 2-pole state-variable filter.
#[derive(Debug, Clone, Copy, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
}

impl Svf {
    #[inline]
    pub fn process(&mut self, c: &Coefficients, x: f32) -> Outputs {
        let v1 = c.a1 * (self.ic1 + c.g * (x - self.ic2));
        let v2 = self.ic2 + c.g * v1;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        Outputs { low: v2, band: v1, high: x - c.k * v1 - v2 }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::response_db;

    fn response(freq: f64, cutoff: f32, pick: fn(Outputs) -> f32) -> f64 {
        let c = Coefficients::new(cutoff, BUTTERWORTH_Q, 48_000.0);
        let mut f = Svf::default();
        response_db(freq, 48_000.0, |block| {
            for s in block.chunks_exact_mut(2) {
                let y = pick(f.process(&c, s[0]));
                s[0] = y;
                s[1] = y;
            }
        })
    }

    #[test]
    fn butterworth_low_and_high_pass() {
        assert!(response(100.0, 1_000.0, |o| o.low).abs() < 0.1, "pass band");
        assert!((response(1_000.0, 1_000.0, |o| o.low) + 3.0).abs() < 0.2, "-3 dB at the cutoff");
        assert!(response(10_000.0, 1_000.0, |o| o.low) < -38.0, "12 dB per octave");
        assert!(response(10_000.0, 1_000.0, |o| o.high).abs() < 0.1);
        assert!(response(100.0, 1_000.0, |o| o.high) < -38.0);
    }
}
