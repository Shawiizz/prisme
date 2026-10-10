//! The one-knob DJ filter: at the centre nothing; turned left a low-pass closing from
//! 20 kHz down to 40 Hz, turned right a high-pass opening from 20 Hz up to 12 kHz. Four
//! poles (24 dB per octave) with a little resonance at the cutoff.

use crate::smooth::Smoothed;
use crate::svf::{Coefficients, Svf};

/// Around the centre the knob does nothing.
const DEAD_ZONE: f32 = 0.03;
const LOW_PASS_HZ: (f32, f32) = (20_000.0, 40.0);
const HIGH_PASS_HZ: (f32, f32) = (20.0, 12_000.0);
/// Q of the two stages: Butterworth on the first, raised on the second for a peak.
const Q: [f32; 2] = [0.54, 1.6];
const GLIDE_S: f32 = 0.02;

pub struct DjFilter {
    sample_rate: f32,
    position: Smoothed,
    /// 0 = bypassed, 1 = filtering: crossfades in and out of the dead zone.
    wet: Smoothed,
    stages: [[Svf; 2]; 2],
}

impl DjFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            position: Smoothed::new(0.0, GLIDE_S, sample_rate),
            wet: Smoothed::new(0.0, GLIDE_S, sample_rate),
            stages: [[Svf::default(); 2]; 2],
        }
    }

    /// -1 = low-pass fully closed, 0 = off, 1 = high-pass fully open.
    pub fn set_position(&mut self, position: f32) {
        let position = position.clamp(-1.0, 1.0);
        self.position.set(position);
        self.wet.set(if position.abs() > DEAD_ZONE { 1.0 } else { 0.0 });
    }

    pub fn position(&self) -> f32 {
        self.position.target()
    }

    /// Processes interleaved stereo in place.
    pub fn process(&mut self, stereo: &mut [f32]) {
        if self.wet.is_settled() && self.wet.target() == 0.0 {
            self.stages = Default::default();
            return;
        }
        for frame in stereo.chunks_exact_mut(2) {
            let position = self.position.tick();
            let wet = self.wet.tick();
            let high_pass = position > 0.0;
            let amount = ((position.abs() - DEAD_ZONE) / (1.0 - DEAD_ZONE)).clamp(0.0, 1.0);
            let (from, to) = if high_pass { HIGH_PASS_HZ } else { LOW_PASS_HZ };
            let cutoff = from * (to / from).powf(amount);
            let c = [
                Coefficients::new(cutoff, Q[0], self.sample_rate),
                Coefficients::new(cutoff, Q[1], self.sample_rate),
            ];
            for (sample, stages) in frame.iter_mut().zip(&mut self.stages) {
                let mut y = *sample;
                for (stage, c) in stages.iter_mut().zip(&c) {
                    let out = stage.process(c, y);
                    y = if high_pass { out.high } else { out.low };
                }
                *sample += (y - *sample) * wet;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::response_db;

    fn response(freq: f64, position: f32) -> f64 {
        let mut f = DjFilter::new(48_000.0);
        f.set_position(position);
        response_db(freq, 48_000.0, |block| f.process(block))
    }

    #[test]
    fn centre_is_transparent() {
        for freq in [50.0, 1_000.0, 10_000.0] {
            assert!(response(freq, 0.0).abs() < 1e-3);
        }
    }

    #[test]
    fn left_closes_a_low_pass_right_opens_a_high_pass() {
        assert!(response(5_000.0, -0.6) < -30.0, "highs gone");
        assert!(response(60.0, -0.6).abs() < 1.0, "bass kept");
        assert!(response(60.0, 0.6) < -30.0, "bass gone");
        assert!(response(8_000.0, 0.6).abs() < 1.0, "highs kept");
    }
}
