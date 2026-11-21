//! Effects that sweep: flanger, phaser and filter, each moved by a slow oscillator whose
//! cycle lasts a few beats. Their second knob: the flanger's width, the phaser's depth, the
//! filter's resonance.

use super::delay::DelayBuffer;
use super::Params;
use crate::svf::{Coefficients, Svf};
use std::f32::consts::{PI, TAU};

/// The oscillator's cycle, in effect periods (a period of one beat sweeps over 4 beats).
const CYCLE_PERIODS: f32 = 4.0;

/// Slow oscillator, 0..1.
#[derive(Default)]
struct Lfo {
    phase: f32,
}

impl Lfo {
    #[inline]
    fn tick(&mut self, p: &Params) -> f32 {
        self.phase = (self.phase + 1.0 / (CYCLE_PERIODS * p.period_s * p.sample_rate).max(1.0)).fract();
        0.5 - 0.5 * (TAU * self.phase).cos()
    }
}

/// The sweep with the width in the middle; turned up, it goes twice as far.
const FLANGER_DELAY_MS: (f32, f32) = (0.5, 5.0);
const FLANGER_FEEDBACK: f32 = 0.85;

pub(crate) struct Flanger {
    buffer: DelayBuffer,
    lfo: Lfo,
    last: [f32; 2],
}

impl Flanger {
    pub fn new(sample_rate: f32) -> Self {
        let longest_ms = FLANGER_DELAY_MS.0 + 2.0 * (FLANGER_DELAY_MS.1 - FLANGER_DELAY_MS.0);
        let frames = (longest_ms / 1_000.0 * sample_rate) as usize + 4;
        Self { buffer: DelayBuffer::new(frames), lfo: Lfo::default(), last: [0.0; 2] }
    }

    pub fn reset(&mut self) {
        self.buffer.reset();
        self.lfo = Lfo::default();
        self.last = [0.0; 2];
    }

    pub fn process(&mut self, stereo: &mut [f32], p: &Params) {
        for (frame, knobs) in stereo.chunks_exact_mut(2).zip(p.knobs) {
            let feedback = knobs.amount * FLANGER_FEEDBACK;
            let sweep = self.lfo.tick(p) * 2.0 * knobs.param2;
            let ms = FLANGER_DELAY_MS.0 + (FLANGER_DELAY_MS.1 - FLANGER_DELAY_MS.0) * sweep;
            let delayed = self.buffer.read(ms / 1_000.0 * p.sample_rate);
            self.buffer.push([frame[0] + self.last[0] * feedback, frame[1] + self.last[1] * feedback]);
            self.last = delayed;
            frame[0] = (frame[0] + delayed[0]) * 0.5;
            frame[1] = (frame[1] + delayed[1]) * 0.5;
        }
    }
}

const PHASER_STAGES: usize = 6;
/// The sweep with the depth in the middle; the depth takes it from half as wide (in octaves,
/// from its low end) to half as wide again.
const PHASER_HZ: (f32, f32) = (200.0, 3_000.0);
const PHASER_FEEDBACK: f32 = 0.7;

#[derive(Default)]
pub(crate) struct Phaser {
    lfo: Lfo,
    stages: [[f32; PHASER_STAGES]; 2],
    last: [f32; 2],
}

impl Phaser {
    pub fn process(&mut self, stereo: &mut [f32], p: &Params) {
        for (frame, knobs) in stereo.chunks_exact_mut(2).zip(p.knobs) {
            let feedback = knobs.amount * PHASER_FEEDBACK;
            let sweep = self.lfo.tick(p) * (0.5 + knobs.param2);
            let hz = PHASER_HZ.0 * (PHASER_HZ.1 / PHASER_HZ.0).powf(sweep);
            let t = (PI * hz / p.sample_rate).tan();
            let a = (t - 1.0) / (t + 1.0);
            for ((sample, stages), last) in frame.iter_mut().zip(&mut self.stages).zip(&mut self.last) {
                let mut y = *sample + *last * feedback;
                for state in stages.iter_mut() {
                    // first-order all-pass
                    let out = a * y + *state;
                    *state = y - a * out;
                    y = out;
                }
                *last = y;
                *sample = (*sample + y) * 0.5;
            }
        }
    }
}

const FILTER_HZ: (f32, f32) = (250.0, 8_000.0);
/// With the resonance in the middle; it goes from a quarter of it to 1.75 times it.
const FILTER_Q: f32 = 2.5;

#[derive(Default)]
pub(crate) struct AutoFilter {
    lfo: Lfo,
    channels: [Svf; 2],
}

impl AutoFilter {
    /// A resonant low-pass sweeping down from the top; the amount is how far down.
    pub fn process(&mut self, stereo: &mut [f32], p: &Params) {
        for (frame, knobs) in stereo.chunks_exact_mut(2).zip(p.knobs) {
            let sweep = 1.0 - (0.2 + 0.8 * knobs.amount) * self.lfo.tick(p);
            let q = FILTER_Q * (0.25 + 1.5 * knobs.param2);
            let hz = FILTER_HZ.0 * (FILTER_HZ.1 / FILTER_HZ.0).powf(sweep);
            let c = Coefficients::new(hz, q, p.sample_rate);
            for (sample, channel) in frame.iter_mut().zip(&mut self.channels) {
                *sample = channel.process(&c, *sample).low;
            }
        }
    }
}
