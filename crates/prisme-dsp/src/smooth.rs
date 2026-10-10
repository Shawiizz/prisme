//! A parameter that glides to its new value instead of jumping (a jump is heard as a
//! click: "zipper noise").

/// One-pole glide: reaches about 99 % of a change in `time_s`.
#[derive(Debug, Clone, Copy)]
pub struct Smoothed {
    value: f32,
    target: f32,
    coef: f32,
}

impl Smoothed {
    pub fn new(value: f32, time_s: f32, sample_rate: f32) -> Self {
        // 99 % after time_s: exp(-4.6) = 0.01
        let coef = 1.0 - (-4.6 / (time_s * sample_rate).max(1.0)).exp();
        Self { value, target: value, coef }
    }

    pub fn set(&mut self, target: f32) {
        self.target = target;
    }

    /// Jumps to the target (when nothing is playing, for instance).
    pub fn reset(&mut self, value: f32) {
        self.value = value;
        self.target = value;
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    /// Whether the value has reached its target (to skip work). Within 1e-4 (-80 dB) of
    /// it: closer, the steps of a slow glide fall below the precision of an `f32`.
    pub fn is_settled(&self) -> bool {
        (self.value - self.target).abs() <= 1e-4 * self.target.abs().max(1e-3)
    }

    /// The value for the next sample.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        self.value += (self.target - self.value) * self.coef;
        if self.is_settled() {
            self.value = self.target;
        }
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glides_then_settles() {
        let mut s = Smoothed::new(0.0, 0.01, 48_000.0);
        s.set(1.0);
        let first = s.tick();
        assert!(first > 0.0 && first < 0.05, "no jump: {first}");
        let after = (0..480).map(|_| s.tick()).last().unwrap();
        assert!((0.98..=1.0).contains(&after), "{after}");
        (0..5_000).for_each(|_| {
            s.tick();
        });
        assert!(s.is_settled());
        assert_eq!(s.tick(), 1.0);
    }
}
