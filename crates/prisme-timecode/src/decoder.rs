//! The decoder: ties conditioning, level monitoring, crossings, speed and position
//! together, one stereo frame at a time.

use crate::conditioning::{Conditioned, Conditioner};
use crate::crossings::{QuadrantGuard, Verdict, ZeroCrossing};
use crate::format::{Channel, SequenceTable, TimecodeFormat};
use crate::level::LevelMonitor;
use crate::position::PositionTracker;
use crate::speed::SpeedEstimator;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// How long a loss of signal is treated as a needle bounce (speed held), in seconds.
const BOUNCE_HOLD_S: f64 = 0.012;
/// Time constant of the fade to zero after that (s).
const STOP_TAU_S: f64 = 0.003;
/// The slow-down rule is suspended this long after dust or a level jump (s).
const QUIET_AFTER_DIRT_S: f64 = 0.004;
/// A bounce ends when coherent crossings agree with the held speed within this ratio.
const BOUNCE_RELEASE_RATIO: f64 = 0.25;

/// Decoder settings.
#[derive(Debug, Clone, Copy)]
pub struct DecoderConfig {
    pub format: &'static TimecodeFormat,
    pub sample_rate: u32,
    /// Set when the turntable's left and right cables are swapped.
    pub swap_channels: bool,
}

/// What the decoder hears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalState {
    /// No usable signal: needle up, record stopped, nothing connected.
    None,
    /// Signal lost for a moment while playing (needle bounce): the speed is held.
    Bounce,
    /// A clean timecode signal.
    Ok,
}

/// Snapshot of the decoder output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecoderStatus {
    /// Speed relative to 33 1/3 rpm (negative = backwards).
    pub speed: f64,
    /// Absolute position on the record, in milliseconds at nominal speed, once locked.
    pub position_ms: Option<f64>,
    pub signal: SignalState,
    /// Level of the carrier (envelope of the secondary channel, full scale = 1.0).
    pub level: f64,
}

/// Timecode vinyl decoder. Feed it the turntable signal with [`TimecodeDecoder::process`]
/// and read [`TimecodeDecoder::status`] whenever needed. Real-time safe once built.
pub struct TimecodeDecoder {
    format: &'static TimecodeFormat,
    sample_rate: f64,
    primary_is_left: bool,
    stop_decay: f64,

    conditioner: Conditioner,
    level: LevelMonitor,
    crossings: [ZeroCrossing; 2],
    guard: QuadrantGuard,
    speed: SpeedEstimator,
    position: PositionTracker,

    n: u64,
    last_theta: f64,
    phase: f64,
    lost_samples: u64,
    last_dirt: f64,
    signal: SignalState,
}

impl TimecodeDecoder {
    /// Builds a decoder. This allocates the shared lookup table on first use, so do it
    /// outside the audio thread.
    pub fn new(config: DecoderConfig) -> Self {
        let format = config.format;
        let sample_rate = config.sample_rate as f64;
        let primary_is_left = (format.primary == Channel::Left) != config.swap_channels;
        Self {
            format,
            sample_rate,
            primary_is_left,
            stop_decay: (-1.0 / (STOP_TAU_S * sample_rate)).exp(),
            conditioner: Conditioner::new(sample_rate),
            level: LevelMonitor::new(sample_rate),
            crossings: [ZeroCrossing::default(); 2],
            guard: QuadrantGuard::default(),
            speed: SpeedEstimator::new(sample_rate, format.resolution),
            position: PositionTracker::new(format, SequenceTable::for_format(format)),
            n: 0,
            last_theta: 0.0,
            phase: 0.0,
            lost_samples: 0,
            last_dirt: f64::NEG_INFINITY,
            signal: SignalState::None,
        }
    }

    pub fn format(&self) -> &'static TimecodeFormat {
        self.format
    }

    /// Feeds interleaved stereo samples (left, right), full scale = 1.0.
    pub fn process(&mut self, stereo: &[f32]) {
        for frame in stereo.chunks_exact(2) {
            let (left, right) = (frame[0] as f64, frame[1] as f64);
            if self.primary_is_left {
                self.process_frame(left, right);
            } else {
                self.process_frame(right, left);
            }
        }
    }

    /// Speed relative to 33 1/3 rpm (negative = backwards).
    pub fn speed(&self) -> f64 {
        self.speed.speed()
    }

    /// Absolute position on the record in milliseconds (at nominal speed), once locked.
    pub fn position_ms(&self) -> Option<f64> {
        self.position.position_cycles(self.phase).map(|c| c * self.format.cycle_ms())
    }

    pub fn status(&self) -> DecoderStatus {
        DecoderStatus {
            speed: self.speed(),
            position_ms: self.position_ms(),
            signal: self.signal,
            level: self.level.amplitude(),
        }
    }

    fn process_frame(&mut self, p_in: f64, s_in: f64) {
        self.n += 1;
        let n = self.n;
        let t = n as f64;

        let (p, s, amp_p, amp_s) = match self.conditioner.process(p_in, s_in) {
            Conditioned::Click => {
                self.last_dirt = t;
                return;
            }
            Conditioned::NoSignal => {
                self.lost_samples += 1;
                if self.lost_samples as f64 > BOUNCE_HOLD_S * self.sample_rate {
                    self.speed.fade(self.stop_decay);
                    self.signal = SignalState::None;
                } else {
                    self.signal = SignalState::Bounce;
                }
                return;
            }
            Conditioned::Clean { p, s, amp_p, amp_s, suspicious } => {
                if suspicious {
                    self.last_dirt = t;
                }
                (p, s, amp_p, amp_s)
            }
        };
        self.lost_samples = 0;

        let steady = !self.speed.is_fast(n);
        let bouncing = self.level.update((p * p + s * s).sqrt(), amp_s, self.speed.speed(), steady, t);
        if self.level.bounce_expired(t, BOUNCE_HOLD_S) {
            self.speed.fade(self.stop_decay);
        }
        self.signal = if bouncing { SignalState::Bounce } else { SignalState::Ok };

        // unwrapped carrier phase, forward positive
        let theta = (p / amp_p).atan2(s / amp_s);
        let mut d = theta - self.last_theta;
        if d > PI {
            d -= TAU;
        } else if d < -PI {
            d += TAU;
        }
        self.last_theta = theta;
        self.phase += d * self.format.phase_sign;

        for (channel, value, amplitude) in [(0usize, p, amp_p), (1usize, s, amp_s)] {
            let Some(crossing_t) = self.crossings[channel].update(value, amplitude, t) else {
                continue;
            };
            let quadrant = (self.phase / FRAC_PI_2).round() as i64;
            match self.guard.check(channel, quadrant, self.speed.speed()) {
                Verdict::Reject => {
                    self.last_dirt = t;
                    continue;
                }
                Verdict::AcceptReversal => self.speed.enter_fast(n),
                Verdict::Accept | Verdict::First => {}
            }
            if let Some(raw) =
                self.speed.on_crossing(crossing_t, quadrant, n, self.level.max_plausible_speed())
            {
                // quiet but coherent crossings: the record is still turning
                let held = self.speed.speed();
                if self.level.is_bouncing() && (raw - held).abs() < BOUNCE_RELEASE_RATIO * held.abs() {
                    self.level.release();
                }
            }
            if channel == 0 {
                self.position.on_primary_crossing(s.abs());
            } else if p > 0.0 {
                self.position.on_bit_crossing(p, self.phase, self.speed.raw() >= 0.0);
            }
        }

        if !bouncing && t - self.last_dirt > QUIET_AFTER_DIRT_S * self.sample_rate {
            self.speed.decay(t, n);
        }
        self.speed.smooth(n);
    }

    /// True once the absolute position is known.
    pub fn is_locked(&self) -> bool {
        self.position.is_locked()
    }
}
