//! The one-knob DJ filter: at the centre nothing; turned left a low-pass closing from
//! 20 kHz down to 40 Hz, turned right a high-pass opening from 20 Hz up to 12 kHz. Four
//! poles (24 dB per octave) with a little resonance at the cutoff.
//!
//! The knob turned across the centre, the low-pass fades out at its open end, the sound let
//! through as it is, and the high-pass fades in from its own (or the other way): the two
//! ends differ enough in phase that going straight from one to the other clicks.

use crate::smooth::Smoothed;
use crate::svf::{Coefficients, Svf};

/// Around the centre the knob does nothing.
const DEAD_ZONE: f32 = 0.03;
const LOW_PASS_HZ: (f32, f32) = (20_000.0, 40.0);
const HIGH_PASS_HZ: (f32, f32) = (20.0, 12_000.0);
/// Q of the two stages: Butterworth on the first, raised on the second for a peak.
const Q: [f32; 2] = [0.54, 1.6];
const GLIDE_S: f32 = 0.02;
/// Faded out this far (-60 dB), the stages turn to the other side.
const QUIET: f32 = 1e-3;

pub struct DjFilter {
    sample_rate: f32,
    position: Smoothed,
    /// 0 = bypassed, 1 = filtering: crossfades in and out of the dead zone, and out of a side
    /// for the other.
    wet: Smoothed,
    stages: [[Svf; 2]; 2],
    /// The side the stages filter on (true: the high-pass), `None` while bypassed.
    high_pass: Option<bool>,
    /// Of the two stages, at the position the knob has glided to.
    coefficients: [Coefficients; 2],
}

impl DjFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            position: Smoothed::new(0.0, GLIDE_S, sample_rate),
            wet: Smoothed::new(0.0, GLIDE_S, sample_rate),
            stages: [[Svf::default(); 2]; 2],
            high_pass: None,
            coefficients: [Coefficients::default(); 2],
        }
    }

    /// -1 = low-pass fully closed, 0 = off, 1 = high-pass fully open.
    pub fn set_position(&mut self, position: f32) {
        self.position.set(position.clamp(-1.0, 1.0));
        self.aim();
    }

    pub fn position(&self) -> f32 {
        self.position.target()
    }

    /// The side the knob is turned to, `None` in the dead zone.
    fn wanted(&self) -> Option<bool> {
        let target = self.position.target();
        (target.abs() > DEAD_ZONE).then_some(target > 0.0)
    }

    /// Filtering when the knob is turned to the side the stages filter on, else fading out.
    fn aim(&mut self) {
        let on = self.wanted().is_some_and(|side| Some(side) == self.high_pass);
        self.wet.set(if on { 1.0 } else { 0.0 });
    }

    /// Processes interleaved stereo in place.
    pub fn process(&mut self, stereo: &mut [f32]) {
        let wanted = self.wanted();
        if wanted.is_none() && self.wet.is_settled() && self.wet.target() == 0.0 {
            self.high_pass = None;
            return;
        }
        for frame in stereo.chunks_exact_mut(2) {
            let moving = !self.position.is_settled();
            let position = self.position.tick();
            let wet = self.wet.tick();
            let turn = wanted.filter(|&side| wet < QUIET && Some(side) != self.high_pass);
            if let Some(side) = turn {
                self.start(side, frame);
            }
            let Some(high_pass) = self.high_pass else { continue };
            if moving || turn.is_some() {
                self.coefficients = coefficients(position, high_pass, self.sample_rate);
            }
            for (sample, stages) in frame.iter_mut().zip(&mut self.stages) {
                let mut y = *sample;
                for (stage, c) in stages.iter_mut().zip(&self.coefficients) {
                    let out = stage.process(c, y);
                    y = if high_pass { out.high } else { out.low };
                }
                *sample += (y - *sample) * wet;
            }
        }
    }

    /// The stages started on a side, before `frame`: as they would be having let the sound
    /// through until now.
    fn start(&mut self, high_pass: bool, frame: &[f32]) {
        self.high_pass = Some(high_pass);
        self.aim();
        for (stages, &x) in self.stages.iter_mut().zip(frame) {
            for stage in stages {
                if high_pass {
                    stage.reset();
                } else {
                    stage.hold(x);
                }
            }
        }
    }
}

/// The coefficients of the stages filtering on a side, with the knob at `position`: at the
/// side's open end while the knob is on the other.
fn coefficients(position: f32, high_pass: bool, sample_rate: f32) -> [Coefficients; 2] {
    let on_side = if high_pass { position } else { -position };
    let amount = ((on_side - DEAD_ZONE) / (1.0 - DEAD_ZONE)).clamp(0.0, 1.0);
    let (from, to) = if high_pass { HIGH_PASS_HZ } else { LOW_PASS_HZ };
    let cutoff = from * (to / from).powf(amount);
    Q.map(|q| Coefficients::new(cutoff, q, sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{response_db, sine};

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

    /// The largest jump from one sample to the next, past the first `skip` frames.
    fn largest_step(stereo: &[f32], skip: usize) -> f32 {
        let left: Vec<f32> = stereo.chunks_exact(2).skip(skip).map(|f| f[0]).collect();
        left.windows(2).fold(0.0, |m, w| m.max((w[1] - w[0]).abs()))
    }

    /// A knob turned fast across the centre and back, as an encoder does: a step of 0.08
    /// (`step` ticks of 0.04 of the knob) every block of 64 frames.
    #[test]
    fn a_knob_turned_fast_across_the_centre_glides_without_a_click() {
        for ticks in [1, 2, 3] {
            let mut signal = sine(220.0, 48_000.0, 48_000);
            let smooth = largest_step(&signal, 0);
            let mut f = DjFilter::new(48_000.0);
            let step = 0.08 * ticks as f32;
            let mut position = -0.8;
            let mut rising = true;
            for block in signal.chunks_mut(128) {
                f.process(block);
                position += if rising { step } else { -step };
                if position.abs() >= 0.8 {
                    rising = !rising;
                }
                f.set_position(position);
            }
            let heard = largest_step(&signal, 4_800);
            assert!(heard < 1.5 * smooth, "{ticks} ticks: {heard} against {smooth}");
        }
    }

    #[test]
    fn a_knob_turned_across_the_centre_filters_on_its_new_side() {
        let mut f = DjFilter::new(48_000.0);
        f.set_position(-0.6);
        f.process(&mut sine(60.0, 48_000.0, 4_800));
        f.set_position(0.6);
        assert!(response_db(60.0, 48_000.0, |block| f.process(block)) < -30.0, "bass gone");
    }

    #[test]
    fn left_closes_a_low_pass_right_opens_a_high_pass() {
        assert!(response(5_000.0, -0.6) < -30.0, "highs gone");
        assert!(response(60.0, -0.6).abs() < 1.0, "bass kept");
        assert!(response(60.0, 0.6) < -30.0, "bass gone");
        assert!(response(8_000.0, 0.6).abs() < 1.0, "highs kept");
    }
}
