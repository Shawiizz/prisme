//! A simulated turntable: generates the timecode signal a real record would produce, so
//! the whole chain can be tried without hardware.

use clap::ValueEnum;
use prisme_timecode::testing::SignalGenerator;
use prisme_timecode::TimecodeFormat;
use std::f64::consts::TAU;

/// Where the simulated needle starts on the record (seconds). Used as lead-in so that
/// the track starts at its beginning in absolute mode.
pub const START_S: f64 = 10.0;

#[derive(Clone, Copy, ValueEnum)]
pub enum Pattern {
    /// Plays at +2% for ever.
    Steady,
    /// Plays, then scratches back and forth every few seconds.
    Scratch,
    /// Plays with small pitch nudges, like when beatmatching by hand.
    Nudge,
}

pub struct SimulatedTurntable {
    generator: SignalGenerator,
    pattern: Pattern,
    sample_rate: f64,
    t: f64,
    buffer: Vec<f32>,
}

impl SimulatedTurntable {
    pub fn new(format: &'static TimecodeFormat, sample_rate: u32, pattern: Pattern) -> Self {
        Self {
            generator: SignalGenerator::new(format, sample_rate, START_S * 1000.0, 600_000.0),
            pattern,
            sample_rate: sample_rate as f64,
            t: 0.0,
            buffer: Vec::with_capacity(16_384 * 2),
        }
    }

    fn speed_at(pattern: Pattern, t: f64) -> f64 {
        match pattern {
            Pattern::Steady => 1.02,
            Pattern::Nudge => 1.0 + if (t % 4.0) < 0.3 { 0.06 } else if (t % 4.0) > 2.0 && (t % 4.0) < 2.3 { -0.06 } else { 0.0 },
            Pattern::Scratch => {
                let cycle = t % 6.0;
                if cycle < 4.0 {
                    1.0
                } else {
                    -1.8 * (TAU * 2.0 * (cycle - 4.0)).sin()
                }
            }
        }
    }

    /// Renders `frames` stereo frames of turntable signal (real-time safe once warmed up).
    pub fn render(&mut self, frames: usize) -> &[f32] {
        self.buffer.clear();
        let (t0, sr, pattern) = (self.t, self.sample_rate, self.pattern);
        self.generator.render(frames, |i| Self::speed_at(pattern, t0 + i as f64 / sr), &mut self.buffer);
        self.t += frames as f64 / sr;
        &self.buffer
    }
}
