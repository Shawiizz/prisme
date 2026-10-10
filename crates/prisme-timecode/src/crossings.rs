//! Zero crossings of the carrier and their validation.
//!
//! At a zero crossing the carrier phase is an exact multiple of 90 degrees, whatever the
//! amplitude of the bits: crossings are the reference points for the speed. Dust adds
//! spurious crossings, which [`QuadrantGuard`] filters out.

/// Hysteresis around zero, relative to the channel envelope.
const HYSTERESIS: f64 = 0.12;
/// Above this speed, an instant change of direction is physically impossible.
const REVERSAL_SPEED: f64 = 0.3;
/// Consecutive backwards steps needed to accept a sudden reversal anyway.
const REVERSAL_VOTES: u32 = 3;

/// Zero crossing detector for one channel, with hysteresis and sub-sample timing.
#[derive(Default, Clone, Copy)]
pub(crate) struct ZeroCrossing {
    prev: f64,
    raw_crossing: f64,
    high: bool,
}

impl ZeroCrossing {
    /// Feeds one sample (`t` = sample index). Returns the interpolated time of the
    /// crossing when the hysteresis confirms one.
    pub(crate) fn update(&mut self, value: f64, amplitude: f64, t: f64) -> Option<f64> {
        if (self.prev <= 0.0) != (value <= 0.0) {
            let frac = if (self.prev - value).abs() > 1e-12 { self.prev / (self.prev - value) } else { 0.5 };
            self.raw_crossing = t - 1.0 + frac;
        }
        self.prev = value;
        let h = HYSTERESIS * amplitude;
        let flipped = if self.high && value < -h {
            self.high = false;
            true
        } else if !self.high && value > h {
            self.high = true;
            true
        } else {
            false
        };
        flipped.then_some(self.raw_crossing)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The first crossing ever: nothing to compare with.
    First,
    /// A genuine crossing.
    Accept,
    /// A genuine, sudden change of direction (confirmed by several crossings).
    AcceptReversal,
    /// Dust or noise.
    Reject,
}

/// Checks that crossings alternate between channels and advance by one quadrant, in a
/// direction compatible with the current speed.
#[derive(Default)]
pub(crate) struct QuadrantGuard {
    last_channel: Option<usize>,
    last_quadrant: i64,
    reverse_votes: u32,
}

impl QuadrantGuard {
    pub(crate) fn check(&mut self, channel: usize, quadrant: i64, speed: f64) -> Verdict {
        let verdict = match self.last_channel {
            None => Verdict::First,
            Some(last) => {
                let step = quadrant - self.last_quadrant;
                let well_formed = last != channel && step.abs() == 1;
                let forward_compatible = speed.abs() < REVERSAL_SPEED || (step as f64).signum() == speed.signum();
                if !well_formed {
                    Verdict::Reject
                } else if forward_compatible {
                    self.reverse_votes = 0;
                    Verdict::Accept
                } else {
                    self.reverse_votes += 1;
                    if self.reverse_votes >= REVERSAL_VOTES {
                        Verdict::AcceptReversal
                    } else {
                        Verdict::Reject
                    }
                }
            }
        };
        self.last_channel = Some(channel);
        self.last_quadrant = quadrant;
        verdict
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossing_time_is_interpolated() {
        let mut zc = ZeroCrossing::default();
        assert_eq!(zc.update(-1.0, 1.0, 1.0), None);
        // crosses zero a quarter of the way between samples 1 and 2
        let t = zc.update(3.0, 1.0, 2.0).unwrap();
        assert!((t - 1.25).abs() < 1e-9);
    }

    #[test]
    fn dust_steps_are_rejected() {
        let mut g = QuadrantGuard::default();
        assert_eq!(g.check(0, 0, 1.0), Verdict::First);
        assert_eq!(g.check(1, 1, 1.0), Verdict::Accept);
        assert_eq!(g.check(1, 2, 1.0), Verdict::Reject, "same channel twice");
        assert_eq!(g.check(0, 1, 1.0), Verdict::Reject, "instant reversal at full speed");
    }

    #[test]
    fn a_confirmed_reversal_is_accepted() {
        let mut g = QuadrantGuard::default();
        g.check(0, 10, 1.0);
        assert_eq!(g.check(1, 9, 1.0), Verdict::Reject);
        assert_eq!(g.check(0, 8, 1.0), Verdict::Reject);
        assert_eq!(g.check(1, 7, 1.0), Verdict::AcceptReversal);
    }
}
