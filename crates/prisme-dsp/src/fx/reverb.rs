//! Reverb after Freeverb (Jezar at Dreampoint, public domain): eight parallel damped comb
//! filters then four series all-passes per channel, the right channel slightly detuned
//! for width. The amount sets the size of the room.

use super::Params;

/// Delay lengths at 44.1 kHz.
const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;
const INPUT_GAIN: f32 = 0.015;
const OUTPUT_GAIN: f32 = 3.0;
const DAMPING: f32 = 0.3;
const ROOM: (f32, f32) = (0.7, 0.98);
const ALLPASS_FEEDBACK: f32 = 0.5;

struct Comb {
    buffer: Vec<f32>,
    index: usize,
    filtered: f32,
}

impl Comb {
    #[inline]
    fn process(&mut self, x: f32, feedback: f32) -> f32 {
        let y = self.buffer[self.index];
        self.filtered = y * (1.0 - DAMPING) + self.filtered * DAMPING;
        self.buffer[self.index] = x + self.filtered * feedback;
        self.index = (self.index + 1) % self.buffer.len();
        y
    }
}

struct Allpass {
    buffer: Vec<f32>,
    index: usize,
}

impl Allpass {
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let delayed = self.buffer[self.index];
        self.buffer[self.index] = x + delayed * ALLPASS_FEEDBACK;
        self.index = (self.index + 1) % self.buffer.len();
        delayed - x
    }
}

struct Channel {
    combs: Vec<Comb>,
    allpasses: Vec<Allpass>,
}

impl Channel {
    fn new(scale: f32, spread: usize) -> Self {
        let length = |l: usize| (((l + spread) as f32 * scale) as usize).max(1);
        Self {
            combs: COMBS
                .iter()
                .map(|&l| Comb { buffer: vec![0.0; length(l)], index: 0, filtered: 0.0 })
                .collect(),
            allpasses: ALLPASSES
                .iter()
                .map(|&l| Allpass { buffer: vec![0.0; length(l)], index: 0 })
                .collect(),
        }
    }

    fn reset(&mut self) {
        for c in &mut self.combs {
            c.buffer.fill(0.0);
            c.filtered = 0.0;
        }
        for a in &mut self.allpasses {
            a.buffer.fill(0.0);
        }
    }

    #[inline]
    fn process(&mut self, x: f32, feedback: f32) -> f32 {
        let mut y: f32 = self.combs.iter_mut().map(|c| c.process(x, feedback)).sum();
        for a in &mut self.allpasses {
            y = a.process(y);
        }
        y
    }
}

pub(crate) struct Reverb {
    channels: [Channel; 2],
}

impl Reverb {
    pub fn new(sample_rate: f32) -> Self {
        let scale = sample_rate / 44_100.0;
        Self { channels: [Channel::new(scale, 0), Channel::new(scale, STEREO_SPREAD)] }
    }

    pub fn reset(&mut self) {
        self.channels.iter_mut().for_each(Channel::reset);
    }

    pub fn process(&mut self, stereo: &mut [f32], p: &Params) {
        let feedback = ROOM.0 + (ROOM.1 - ROOM.0) * p.amount;
        for frame in stereo.chunks_exact_mut(2) {
            let input = (frame[0] + frame[1]) * INPUT_GAIN;
            frame[0] = self.channels[0].process(input, feedback) * OUTPUT_GAIN;
            frame[1] = self.channels[1].process(input, feedback) * OUTPUT_GAIN;
        }
    }
}
