//! Speed estimation from carrier zero crossings.
//!
//! The speed is the number of quadrants travelled divided by the time they took,
//! measured from crossing to crossing (one full cycle when steady, half a cycle when
//! scratching). A median rejects isolated outliers, and an adaptive smoothing is calm
//! while steady and nearly instantaneous once a change is confirmed by 3 crossings.

const EVENTS: usize = 16;
/// Smoothing time constants (s).
const STEADY_TAU_S: f64 = 0.030;
const FAST_TAU_S: f64 = 0.0008;
/// How long the fast mode lasts after the last confirmed change (s).
const FAST_HOLD_S: f64 = 0.03;
/// A change bigger than this (+ relative part) counts towards the fast mode.
const CHANGE_ABS: f64 = 0.008;
const CHANGE_REL: f64 = 0.01;
const CHANGES_TO_CONFIRM: u32 = 3;
/// A hand cannot change the record speed faster than this (per second).
const MAX_ACCEL: f64 = 400.0;
/// The next crossing is overdue (the record slows down) after this many expected intervals.
const OVERDUE: f64 = 1.6;

pub(crate) struct SpeedEstimator {
    sample_rate: f64,
    resolution: f64,
    event_t: [f64; EVENTS],
    event_q: [i64; EVENTS],
    event_pos: usize,
    event_count: usize,
    last_event_t: f64,
    recent: [f64; 5],
    raw: f64,
    speed: f64,
    fast_until: u64,
    changes: u32,
}

impl SpeedEstimator {
    pub(crate) fn new(sample_rate: f64, resolution: f64) -> Self {
        Self {
            sample_rate,
            resolution,
            event_t: [0.0; EVENTS],
            event_q: [0; EVENTS],
            event_pos: 0,
            event_count: 0,
            last_event_t: 0.0,
            recent: [0.0; 5],
            raw: 0.0,
            speed: 0.0,
            fast_until: 0,
            changes: 0,
        }
    }

    /// Smoothed speed (1.0 = nominal, negative = backwards).
    pub(crate) fn speed(&self) -> f64 {
        self.speed
    }

    /// Latest unsmoothed estimate; its sign gives the current direction.
    pub(crate) fn raw(&self) -> f64 {
        self.raw
    }

    pub(crate) fn is_fast(&self, n: u64) -> bool {
        n < self.fast_until
    }

    pub(crate) fn enter_fast(&mut self, n: u64) {
        self.fast_until = n + (FAST_HOLD_S * self.sample_rate) as u64;
    }

    /// A validated crossing at time `t` (samples) in quadrant `quadrant`. Returns the
    /// new raw estimate if one was produced.
    pub(crate) fn on_crossing(&mut self, t: f64, quadrant: i64, n: u64, max_plausible: Option<f64>) -> Option<f64> {
        self.event_t[self.event_pos] = t;
        self.event_q[self.event_pos] = quadrant;
        self.event_pos = (self.event_pos + 1) % EVENTS;
        self.event_count = (self.event_count + 1).min(EVENTS);
        self.last_event_t = t;

        let fast = self.is_fast(n);
        let windows: &[usize] = if fast { &[2, 1] } else { &[4, 2, 1] };
        for &back in windows {
            if self.event_count <= back {
                continue;
            }
            let i = (self.event_pos + EVENTS - 1 - back) % EVENTS;
            let dq = quadrant - self.event_q[i];
            let dt = t - self.event_t[i];
            if dq == 0 || dt <= 0.0 {
                continue;
            }
            let estimate = dq as f64 * 0.25 / (dt / self.sample_rate) / self.resolution;
            if let Some(max) = max_plausible {
                if estimate.abs() > max {
                    return None; // a weak signal cannot come from a fast record
                }
            }
            self.recent = [self.recent[1], self.recent[2], self.recent[3], self.recent[4], estimate];
            let mut window = [0.0f64; 5];
            let used = if fast { 3 } else { 5 };
            window[..used].copy_from_slice(&self.recent[5 - used..]);
            let window = &mut window[..used];
            window.sort_by(|a, b| a.total_cmp(b));
            self.raw = window[used / 2];

            if (self.raw - self.speed).abs() > CHANGE_ABS + CHANGE_REL * self.speed.abs() {
                self.changes += 1;
                if self.changes >= CHANGES_TO_CONFIRM {
                    self.enter_fast(n);
                }
            } else {
                self.changes = 0;
            }
            return Some(self.raw);
        }
        None
    }

    /// When the record slows down, crossings get rare: the time since the last one
    /// bounds the speed. Only applied when the next crossing is clearly overdue.
    pub(crate) fn decay(&mut self, t: f64, n: u64) {
        let raw = self.raw.abs();
        if raw <= 0.0 {
            return;
        }
        let since = (t - self.last_event_t) / self.sample_rate;
        let quarter = 0.25 / (raw * self.resolution);
        if since > OVERDUE * quarter {
            self.raw = self.raw.signum() * 0.25 / since / self.resolution;
            self.enter_fast(n);
        }
    }

    /// Moves the output towards the raw estimate (once per sample).
    pub(crate) fn smooth(&mut self, n: u64) {
        let tau = if self.is_fast(n) { FAST_TAU_S } else { STEADY_TAU_S };
        let a = 1.0 - (-1.0 / (tau * self.sample_rate)).exp();
        let max_step = MAX_ACCEL / self.sample_rate;
        self.speed += (a * (self.raw - self.speed)).clamp(-max_step, max_step);
    }

    /// The record stopped (or the needle was lifted): fade the speed out.
    pub(crate) fn fade(&mut self, factor: f64) {
        self.speed *= factor;
        self.raw = 0.0;
        self.event_count = 0;
    }
}
