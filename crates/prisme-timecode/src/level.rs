//! Signal level monitoring: needle bounces and the level / speed model.
//!
//! A magnetic cartridge is velocity sensitive: the signal level is roughly proportional
//! to the record speed. This module learns that ratio during steady play, which lets the
//! decoder reject crossings that a weak signal cannot produce (noise while the record
//! passes through zero), and it recognises needle bounces (sudden level collapse while
//! playing) so that the speed is held instead of dropping.

/// A level below this fraction of the recent peak is a sudden collapse.
const COLLAPSE_RATIO: f64 = 0.2;
/// The bounce ends when the level is back above this fraction of the level before it.
const RECOVERY_RATIO: f64 = 0.5;
/// Peak hold time constant (s).
const PEAK_TAU_S: f64 = 0.002;
/// Slow level time constant (s).
const SLOW_TAU_S: f64 = 0.03;
/// Learning rate of the level / speed ratio (per sample).
const RATIO_LEARNING: f64 = 0.0005;
/// The ratio is trusted after this much steady play (s).
const RATIO_TRUSTED_AFTER_S: f64 = 0.3;
/// Tolerance of the level / speed check.
const PLAUSIBLE_FACTOR: f64 = 2.5;
const PLAUSIBLE_MARGIN: f64 = 0.15;

pub(crate) struct LevelMonitor {
    sample_rate: f64,
    peak_decay: f64,
    slow_alpha: f64,
    recent_peak: f64,
    slow_level: f64,
    /// Level before the collapse, or 0 when there is no ongoing bounce.
    bounce_ref: f64,
    bounce_start: f64,
    level_per_speed: f64,
    learned_samples: u64,
    amplitude: f64,
}

impl LevelMonitor {
    pub(crate) fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            peak_decay: (-1.0 / (PEAK_TAU_S * sample_rate)).exp(),
            slow_alpha: 1.0 - (-1.0 / (SLOW_TAU_S * sample_rate)).exp(),
            recent_peak: 0.0,
            slow_level: 0.0,
            bounce_ref: 0.0,
            bounce_start: 0.0,
            level_per_speed: 0.0,
            learned_samples: 0,
            amplitude: 0.0,
        }
    }

    /// Updates the monitor with one clean frame. `level` is the instantaneous level
    /// (length of the stereo vector), `amp_s` the secondary envelope, `t` the sample
    /// index. Returns true while a needle bounce is in progress.
    pub(crate) fn update(&mut self, level: f64, amp_s: f64, speed: f64, steady: bool, t: f64) -> bool {
        self.recent_peak = (self.recent_peak * self.peak_decay).max(level);
        if self.bounce_ref == 0.0 {
            let abrupt =
                level < COLLAPSE_RATIO * self.recent_peak && self.recent_peak > 0.6 * self.slow_level;
            if abrupt && speed.abs() > 0.3 {
                self.bounce_ref = self.slow_level;
                self.bounce_start = t;
            }
            self.slow_level += self.slow_alpha * (level - self.slow_level);
        } else if level > RECOVERY_RATIO * self.bounce_ref {
            self.bounce_ref = 0.0;
        }
        let bouncing = self.bounce_ref != 0.0;
        if steady && !bouncing && speed.abs() > 0.5 {
            let ratio = amp_s / speed.abs();
            self.level_per_speed += RATIO_LEARNING * (ratio - self.level_per_speed);
            self.learned_samples = (self.learned_samples + 1).min(u64::MAX / 2);
        }
        self.amplitude = amp_s;
        bouncing
    }

    /// True when a bounce has lasted longer than `hold_s`: the record was stopped.
    pub(crate) fn bounce_expired(&self, t: f64, hold_s: f64) -> bool {
        self.bounce_ref != 0.0 && t - self.bounce_start > hold_s * self.sample_rate
    }

    pub(crate) fn is_bouncing(&self) -> bool {
        self.bounce_ref != 0.0
    }

    /// Ends the bounce: coherent crossings show the record is still turning.
    pub(crate) fn release(&mut self) {
        self.bounce_ref = 0.0;
    }

    /// Highest speed the current level can plausibly come from, once learned.
    pub(crate) fn max_plausible_speed(&self) -> Option<f64> {
        let trusted = self.learned_samples as f64 > RATIO_TRUSTED_AFTER_S * self.sample_rate;
        if self.bounce_ref == 0.0 && trusted && self.level_per_speed > 0.0 {
            Some(PLAUSIBLE_FACTOR * self.amplitude / self.level_per_speed + PLAUSIBLE_MARGIN)
        } else {
            None
        }
    }

    /// Secondary channel envelope of the last clean frame.
    pub(crate) fn amplitude(&self) -> f64 {
        self.amplitude
    }
}
