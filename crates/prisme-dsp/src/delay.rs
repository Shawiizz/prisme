//! A fixed delay for interleaved stereo, to keep signals aligned with one that went
//! through a processor with latency.

pub struct DelayLine {
    buffer: Vec<[f32; 2]>,
    position: usize,
}

impl DelayLine {
    pub fn new(frames: usize) -> Self {
        Self { buffer: vec![[0.0; 2]; frames.max(1)], position: 0 }
    }

    pub fn frames(&self) -> usize {
        self.buffer.len()
    }

    pub fn clear(&mut self) {
        self.buffer.fill([0.0; 2]);
    }

    /// Delays interleaved stereo in place.
    pub fn process(&mut self, stereo: &mut [f32]) {
        for frame in stereo.chunks_exact_mut(2) {
            let delayed = std::mem::replace(&mut self.buffer[self.position], [frame[0], frame[1]]);
            frame[0] = delayed[0];
            frame[1] = delayed[1];
            self.position = (self.position + 1) % self.buffer.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_by_its_length() {
        let mut d = DelayLine::new(2);
        let mut s = [1.0, -1.0, 2.0, -2.0, 3.0, -3.0];
        d.process(&mut s);
        assert_eq!(s, [0.0, 0.0, 0.0, 0.0, 1.0, -1.0]);
    }
}
