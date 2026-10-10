//! Repeats on the beat: echo (both sides together) and ping-pong (alternating sides).
//! Each repeat goes through a gentle low-pass, so that it gets duller, like tape.

use super::delay::DelayBuffer;
use super::Params;
use crate::smooth::Smoothed;

const MAX_DELAY_S: f32 = 4.0;
const MAX_FEEDBACK: f32 = 0.92;
/// Darkening of each repeat (one-pole coefficient).
const TAPE: f32 = 0.35;
/// The delay follows tempo changes over this time (a glide, not a jump).
const DELAY_GLIDE_S: f32 = 0.05;

pub(crate) struct Echo {
    buffer: DelayBuffer,
    delay: Smoothed,
    tone: [f32; 2],
}

impl Echo {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            buffer: DelayBuffer::new((MAX_DELAY_S * sample_rate) as usize + 2),
            delay: Smoothed::new(0.0, DELAY_GLIDE_S, sample_rate),
            tone: [0.0; 2],
        }
    }

    pub fn reset(&mut self) {
        self.buffer.reset();
        self.tone = [0.0; 2];
    }

    pub fn process(&mut self, stereo: &mut [f32], p: &Params) {
        let feedback = p.amount * MAX_FEEDBACK;
        set_delay(&mut self.delay, p, self.buffer.capacity());
        for frame in stereo.chunks_exact_mut(2) {
            // read before writing: the newest frame in the buffer is already one frame old
            let delayed = self.buffer.read(self.delay.tick() - 1.0);
            for (tone, d) in self.tone.iter_mut().zip(delayed) {
                *tone += (d - *tone) * (1.0 - TAPE);
            }
            self.buffer.push([frame[0] + self.tone[0] * feedback, frame[1] + self.tone[1] * feedback]);
            frame[0] = delayed[0];
            frame[1] = delayed[1];
        }
    }
}

/// The first repeat on the left, the next on the right, and so on.
pub(crate) struct PingPong {
    buffer: DelayBuffer,
    delay: Smoothed,
    tone: [f32; 2],
}

impl PingPong {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            buffer: DelayBuffer::new((MAX_DELAY_S * sample_rate) as usize + 2),
            delay: Smoothed::new(0.0, DELAY_GLIDE_S, sample_rate),
            tone: [0.0; 2],
        }
    }

    pub fn reset(&mut self) {
        self.buffer.reset();
        self.tone = [0.0; 2];
    }

    pub fn process(&mut self, stereo: &mut [f32], p: &Params) {
        let feedback = p.amount * MAX_FEEDBACK;
        set_delay(&mut self.delay, p, self.buffer.capacity());
        for frame in stereo.chunks_exact_mut(2) {
            // read before writing: the newest frame in the buffer is already one frame old
            let delayed = self.buffer.read(self.delay.tick() - 1.0);
            for (tone, d) in self.tone.iter_mut().zip(delayed) {
                *tone += (d - *tone) * (1.0 - TAPE);
            }
            let mono = (frame[0] + frame[1]) * 0.5;
            // each side feeds the other
            self.buffer.push([mono + self.tone[1] * feedback, self.tone[0] * feedback]);
            frame[0] = delayed[0];
            frame[1] = delayed[1];
        }
    }
}

fn set_delay(delay: &mut Smoothed, p: &Params, capacity: usize) {
    let frames = (p.period_s * p.sample_rate).clamp(1.0, (capacity - 3) as f32);
    if delay.target() == 0.0 {
        delay.reset(frames);
    } else {
        delay.set(frames);
    }
}
