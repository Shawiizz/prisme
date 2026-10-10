//! Rhythmic and lo-fi effects: the gate cuts the sound on the beat, the bitcrusher
//! lowers its resolution.

use super::Params;

/// Open for this part of each period.
const OPEN: f32 = 0.5;
/// Edges of the gate (seconds): sharp but without clicks.
const EDGE_S: f32 = 0.002;

#[derive(Default)]
pub(crate) struct Gate {
    /// Position within the period, 0..1.
    phase: f32,
    gain: f32,
}

impl Gate {
    pub fn process(&mut self, stereo: &mut [f32], p: &Params) {
        let period = (p.period_s * p.sample_rate).max(1.0);
        if let Some(beat_phase) = p.phase {
            // follow the beat: periods of a beat or less start on it
            let beats_per_period = p.period_s / p.beat_s;
            if beats_per_period <= 1.0 {
                self.phase = (beat_phase / beats_per_period).fract();
            }
        }
        let closed = 1.0 - p.amount;
        let edge = 1.0 / (EDGE_S * p.sample_rate).max(1.0);
        for frame in stereo.chunks_exact_mut(2) {
            let target = if self.phase < OPEN { 1.0 } else { closed };
            self.gain += (target - self.gain).clamp(-edge, edge);
            frame[0] *= self.gain;
            frame[1] *= self.gain;
            self.phase = (self.phase + 1.0 / period).fract();
        }
    }
}

const BITS: (f32, f32) = (16.0, 4.0);
const MAX_HOLD: f32 = 16.0;

#[derive(Default)]
pub(crate) struct Bitcrusher {
    held: [f32; 2],
    count: f32,
}

impl Bitcrusher {
    pub fn process(&mut self, stereo: &mut [f32], p: &Params) {
        let levels = 2f32.powf(BITS.0 + (BITS.1 - BITS.0) * p.amount);
        let hold = 1.0 + (MAX_HOLD - 1.0) * p.amount;
        for frame in stereo.chunks_exact_mut(2) {
            if self.count <= 0.0 {
                self.held = [(frame[0] * levels).round() / levels, (frame[1] * levels).round() / levels];
                self.count += hold;
            }
            self.count -= 1.0;
            frame[0] = self.held[0];
            frame[1] = self.held[1];
        }
    }
}
