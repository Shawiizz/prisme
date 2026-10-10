//! Input conditioning: DC / rumble removal, signal envelopes and click rejection.

/// Below this amplitude (about -50 dBFS) there is no usable signal.
const MIN_AMPLITUDE: f64 = 0.003;
/// A sample above this multiple of the envelope is a click (dust, scratch on the vinyl).
const CLICK_FACTOR: f64 = 2.2;
/// Clicks are only detected once the envelope has been established for this long (s).
const CLICK_WARMUP_S: f64 = 0.01;
/// A "click" lasting longer than this (s) is a level change, e.g. the end of a dropout.
const MAX_CLICK_S: f64 = 0.0015;
/// Envelope time constant (s): about 1.5 carrier cycles.
const ENVELOPE_TAU_S: f64 = 0.0015;
/// DC / rumble high-pass corner (Hz).
const DC_CORNER_HZ: f64 = 5.0;

/// Result of conditioning one stereo frame.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Conditioned {
    /// Nothing usable (needle up, record stopped, no input).
    NoSignal,
    /// A click: ignore this frame entirely.
    Click,
    /// A usable frame. `suspicious` is set when the level just jumped (end of a click
    /// burst treated as a level change).
    Clean { p: f64, s: f64, amp_p: f64, amp_s: f64, suspicious: bool },
}

pub(crate) struct Conditioner {
    sample_rate: f64,
    dc_alpha: f64,
    env_alpha: f64,
    dc_p: f64,
    dc_s: f64,
    pow_p: f64,
    pow_s: f64,
    warm_samples: u64,
    click_run: u64,
}

impl Conditioner {
    pub(crate) fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            dc_alpha: 1.0 - (-std::f64::consts::TAU * DC_CORNER_HZ / sample_rate).exp(),
            env_alpha: 1.0 - (-1.0 / (ENVELOPE_TAU_S * sample_rate)).exp(),
            dc_p: 0.0,
            dc_s: 0.0,
            pow_p: 0.0,
            pow_s: 0.0,
            warm_samples: 0,
            click_run: 0,
        }
    }

    /// `p_in` / `s_in`: primary and secondary channel samples, full scale = 1.0.
    pub(crate) fn process(&mut self, p_in: f64, s_in: f64) -> Conditioned {
        self.dc_p += self.dc_alpha * (p_in - self.dc_p);
        self.dc_s += self.dc_alpha * (s_in - self.dc_s);
        let p = p_in - self.dc_p;
        let s = s_in - self.dc_s;
        // RMS * sqrt(2) = amplitude of a sine
        let amp_p = (2.0 * self.pow_p).sqrt();
        let amp_s = (2.0 * self.pow_s).sqrt();

        // Update the envelopes; a click is clipped so that it cannot inflate them.
        let present = amp_p > MIN_AMPLITUDE && amp_s > MIN_AMPLITUDE;
        let (lim_p, lim_s) = if present {
            ((CLICK_FACTOR * amp_p).powi(2), (CLICK_FACTOR * amp_s).powi(2))
        } else {
            (f64::MAX, f64::MAX)
        };
        self.pow_p += self.env_alpha * ((p * p).min(lim_p) - self.pow_p);
        self.pow_s += self.env_alpha * ((s * s).min(lim_s) - self.pow_s);
        self.warm_samples = if present { self.warm_samples + 1 } else { 0 };

        let mut suspicious = false;
        let warmed_up = self.warm_samples as f64 > CLICK_WARMUP_S * self.sample_rate;
        if warmed_up && (p.abs() > CLICK_FACTOR * amp_p || s.abs() > CLICK_FACTOR * amp_s) {
            self.click_run += 1;
            if (self.click_run as f64) < MAX_CLICK_S * self.sample_rate {
                return Conditioned::Click;
            }
            // too long for a click: the level changed, follow it
            self.pow_p = p * p;
            self.pow_s = s * s;
            suspicious = true;
        }
        self.click_run = 0;

        if !present {
            return Conditioned::NoSignal;
        }
        Conditioned::Clean { p, s, amp_p, amp_s, suspicious }
    }
}
