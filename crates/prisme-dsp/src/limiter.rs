//! Master limiter: no sample leaves above the threshold, without the distortion of
//! clipping. It looks ahead: the signal is delayed by a short window during which the
//! gain is brought down smoothly before a peak, then released slowly after it. Switched
//! off, it is bypassed altogether (no delay).
//!
//! Gain: the smallest gain any sample of the window needs (sliding minimum), released
//! with a one-pole, then averaged over the window (box filter) so that it reaches that
//! minimum exactly when the peak comes out of the delay.

use crate::delay::DelayLine;

/// Short: it adds to the latency of the whole software mixer.
const LOOKAHEAD_S: f32 = 0.001;
const RELEASE_S: f32 = 0.15;

pub struct Limiter {
    threshold: f32,
    enabled: bool,
    window: usize,
    delay: DelayLine,
    /// Sliding minimum of the gains needed: (position, gain), increasing gains.
    minimum: Vec<(usize, f32)>,
    minimum_head: usize,
    minimum_len: usize,
    released: f32,
    release: f32,
    boxed: Vec<f32>,
    boxed_sum: f64,
    position: usize,
}

impl Limiter {
    /// `threshold`: highest output level (linear).
    pub fn new(sample_rate: f32, threshold: f32) -> Self {
        let window = ((LOOKAHEAD_S * sample_rate) as usize).max(1);
        Self {
            threshold,
            enabled: true,
            window,
            delay: DelayLine::new(window),
            minimum: vec![(0, 1.0); window + 1],
            minimum_head: 0,
            minimum_len: 0,
            released: 1.0,
            release: 1.0 - (-1.0 / (RELEASE_S * sample_rate)).exp(),
            boxed: vec![1.0; window],
            boxed_sum: window as f64,
            position: 0,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if enabled && !self.enabled {
            self.delay.clear();
        }
        self.enabled = enabled;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Delay the limiter adds (frames), 0 when off.
    pub fn latency(&self) -> usize {
        if self.enabled {
            self.window
        } else {
            0
        }
    }

    /// Processes interleaved stereo in place.
    pub fn process(&mut self, stereo: &mut [f32]) {
        if !self.enabled {
            return;
        }
        for frame in stereo.chunks_exact_mut(2) {
            let peak = frame[0].abs().max(frame[1].abs());
            let needed = if peak > self.threshold { self.threshold / peak } else { 1.0 };
            let lowest = self.push_minimum(needed);
            self.released = if lowest < self.released {
                lowest
            } else {
                self.released + (lowest - self.released) * self.release
            };
            let slot = self.position % self.window;
            self.boxed_sum += (self.released - self.boxed[slot]) as f64;
            self.boxed[slot] = self.released;
            let gain = (self.boxed_sum / self.window as f64) as f32;

            self.delay.process(frame);
            frame[0] *= gain;
            frame[1] *= gain;
            self.position += 1;
        }
    }

    /// Adds the gain needed now; returns the smallest over the last `window + 1` samples.
    fn push_minimum(&mut self, gain: f32) -> f32 {
        let capacity = self.minimum.len();
        let oldest = self.position.saturating_sub(self.window);
        while self.minimum_len > 0 && self.minimum[self.minimum_head].0 < oldest {
            self.minimum_head = (self.minimum_head + 1) % capacity;
            self.minimum_len -= 1;
        }
        while self.minimum_len > 0 {
            let last = (self.minimum_head + self.minimum_len - 1) % capacity;
            if self.minimum[last].1 < gain {
                break;
            }
            self.minimum_len -= 1;
        }
        let tail = (self.minimum_head + self.minimum_len) % capacity;
        self.minimum[tail] = (self.position, gain);
        self.minimum_len += 1;
        self.minimum[self.minimum_head].1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::sine;

    #[test]
    fn off_is_a_bypass() {
        let mut l = Limiter::new(48_000.0, 0.5);
        l.set_enabled(false);
        let mut s = [2.0, -2.0, 1.0, 1.0];
        l.process(&mut s);
        assert_eq!((s, l.latency()), ([2.0, -2.0, 1.0, 1.0], 0));
    }

    #[test]
    fn nothing_goes_above_the_threshold() {
        let mut l = Limiter::new(48_000.0, 0.9);
        let mut loud: Vec<f32> = sine(100.0, 48_000.0, 48_000).iter().map(|s| s * 3.0).collect();
        // a sudden spike as well
        loud[60_000] = 8.0;
        for block in loud.chunks_mut(128) {
            l.process(block);
        }
        let peak = loud.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak <= 0.9 + 1e-4, "{peak}");
    }

    #[test]
    fn quiet_audio_only_gets_delayed() {
        let mut l = Limiter::new(48_000.0, 0.9);
        let input: Vec<f32> = sine(440.0, 48_000.0, 4_800).iter().map(|s| s * 0.5).collect();
        let mut output = input.clone();
        l.process(&mut output);
        let d = l.latency() * 2;
        assert!(output[d..].iter().zip(&input).all(|(o, i)| (o - i).abs() < 1e-6));
    }
}
