/// Stereo memory of the last frames, read at any (fractional) delay.
pub(crate) struct DelayBuffer {
    frames: Vec<[f32; 2]>,
    /// Where the next frame goes.
    next: usize,
    /// Frames pushed since the last reset, up to the capacity: older ones read as silence.
    filled: usize,
}

impl DelayBuffer {
    pub fn new(capacity: usize) -> Self {
        Self { frames: vec![[0.0; 2]; capacity.max(4)], next: 0, filled: 0 }
    }

    /// Forgets the past at no cost: the buffer is seconds long, too much to clear in an
    /// audio callback.
    pub fn reset(&mut self) {
        self.filled = 0;
    }

    pub fn capacity(&self) -> usize {
        self.frames.len()
    }

    pub fn push(&mut self, frame: [f32; 2]) {
        self.frames[self.next] = frame;
        self.next = (self.next + 1) % self.frames.len();
        self.filled = (self.filled + 1).min(self.frames.len());
    }

    /// The frame pushed ``delay`` frames before the last one, between frames linearly.
    pub fn read(&self, delay: f32) -> [f32; 2] {
        let len = self.frames.len();
        let delay = delay.clamp(0.0, (len - 2) as f32);
        let whole = delay as usize;
        let frac = delay - whole as f32;
        let at = |d: usize| {
            if d < self.filled {
                self.frames[(self.next + 2 * len - 1 - d) % len]
            } else {
                [0.0; 2]
            }
        };
        let (a, b) = (at(whole), at(whole + 1));
        [a[0] + (b[0] - a[0]) * frac, a[1] + (b[1] - a[1]) * frac]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_back_in_time() {
        let mut d = DelayBuffer::new(8);
        for i in 0..5 {
            d.push([i as f32, -(i as f32)]);
        }
        assert_eq!(d.read(0.0), [4.0, -4.0]);
        assert_eq!(d.read(2.0), [2.0, -2.0]);
        assert_eq!(d.read(1.5), [2.5, -2.5]);
    }

    #[test]
    fn a_reset_buffer_is_silent() {
        let mut d = DelayBuffer::new(8);
        for i in 0..8 {
            d.push([1.0 + i as f32; 2]);
        }
        d.reset();
        d.push([9.0; 2]);
        assert_eq!(d.read(0.0), [9.0; 2]);
        assert_eq!(d.read(0.5), [4.5; 2]);
        assert_eq!(d.read(3.0), [0.0; 2]);
    }
}
