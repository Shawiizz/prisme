//! Synthetic timecode signals for tests, benchmarks and simulations.
//!
//! [`SignalGenerator`] produces the signal of a control record turning at any speed or
//! direction; [`Degradation`] adds what real records and needles add (noise, dust,
//! dropouts, channel imbalance).

use crate::format::TimecodeFormat;
use std::f64::consts::TAU;

/// Bit amplitudes on the primary channel (relative to full scale).
const HIGH_BIT: f64 = 0.80;
const LOW_BIT: f64 = 0.40;
/// Secondary channel amplitude relative to the primary. Both channels carry the bits, as
/// on real records.
const SECONDARY: f64 = 0.75;
/// Speed at which the level stops growing.
const MAX_LEVEL_SPEED: f64 = 1.6;

/// Generator of the signal of a turning control record.
pub struct SignalGenerator {
    format: &'static TimecodeFormat,
    sample_rate: f64,
    bits: Vec<u8>,
    position: f64,
    /// When false, the level is constant whatever the speed (by default it follows the
    /// speed, like a magnetic cartridge).
    pub velocity_sensitive: bool,
}

impl SignalGenerator {
    /// `start_ms`: position on the record (milliseconds at nominal speed).
    /// `span_ms`: how much of the record to prepare (memory: one byte per cycle).
    pub fn new(format: &'static TimecodeFormat, sample_rate: u32, start_ms: f64, span_ms: f64) -> Self {
        let cycles = ((start_ms + span_ms) / format.cycle_ms()).ceil() as usize + 2;
        let cycles = cycles.min(format.length as usize);
        let mut bits = vec![0u8; cycles];
        let mut state = format.seed;
        for bit in bits.iter_mut().skip(1) {
            state = format.next_state(state);
            *bit = ((state >> (format.bits - 1)) & 1) as u8;
        }
        Self {
            format,
            sample_rate: sample_rate as f64,
            bits,
            position: start_ms / format.cycle_ms(),
            velocity_sensitive: true,
        }
    }

    /// Position on the record in milliseconds (at nominal speed).
    pub fn position_ms(&self) -> f64 {
        self.position * self.format.cycle_ms()
    }

    /// Moves the needle (needle drop).
    pub fn jump_ms(&mut self, delta_ms: f64) {
        self.position = (self.position + delta_ms / self.format.cycle_ms()).max(2.0);
    }

    /// Appends `frames` interleaved stereo frames (left, right) to `out`. `speed_of(i)`
    /// gives the record speed for frame `i` (1.0 = nominal, negative = backwards).
    pub fn render(&mut self, frames: usize, mut speed_of: impl FnMut(usize) -> f64, out: &mut Vec<f32>) {
        out.reserve(frames * 2);
        let last = self.bits.len() as isize - 1;
        for i in 0..frames {
            let speed = speed_of(i);
            let cycle = self.position.floor();
            let index = (cycle as isize).clamp(1, last) as usize;
            let amplitude = if self.bits[index] == 1 { HIGH_BIT } else { LOW_BIT };
            let (sin, cos) = (TAU * (self.position - cycle)).sin_cos();
            let level = if self.velocity_sensitive { speed.abs().min(MAX_LEVEL_SPEED) } else { 1.0 };
            // as on real records: a cycle runs from one primary peak to the next, and the
            // secondary lags the primary by a quarter of a cycle
            let primary = level * amplitude * cos;
            let secondary = level * amplitude * SECONDARY * sin;
            let (left, right) = match self.format.primary {
                crate::format::Channel::Right => (secondary, primary),
                crate::format::Channel::Left => (primary, secondary),
            };
            out.push(left as f32);
            out.push(right as f32);
            self.position = (self.position + speed * self.format.resolution / self.sample_rate).max(2.0);
        }
    }
}

/// Small deterministic random generator (xorshift64*).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    /// Uniform in [0, 1).
    pub fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Approximately normal, unit variance-ish (sum of uniforms).
    pub fn gauss(&mut self) -> f64 {
        (0..6).map(|_| self.uniform()).sum::<f64>() - 3.0
    }
}

/// Imperfections of real records, needles and cables.
#[derive(Debug, Clone, Copy, Default)]
pub struct Degradation {
    /// Gain applied to the signal (0 = unchanged).
    pub gain: f64,
    /// White noise level, relative to full scale.
    pub noise: f64,
    /// Crackles (dust) per second.
    pub crackles_per_s: f64,
    /// Level of the right channel relative to the left one (0 = unchanged).
    pub imbalance: f64,
    /// Short dropouts (needle bounces) per second, each `dropout_ms` long.
    pub dropouts_per_s: f64,
    pub dropout_ms: f64,
}

/// Applies a [`Degradation`] to a stream of stereo frames.
pub struct Degrader {
    config: Degradation,
    rng: Rng,
    sample_rate: f64,
    crackle_left: usize,
    crackle_amplitude: f64,
    dropout_left: usize,
}

impl Degrader {
    pub fn new(config: Degradation, sample_rate: u32, seed: u64) -> Self {
        Self {
            config,
            rng: Rng::new(seed),
            sample_rate: sample_rate as f64,
            crackle_left: 0,
            crackle_amplitude: 0.0,
            dropout_left: 0,
        }
    }

    pub fn apply(&mut self, stereo: &mut [f32]) {
        let c = self.config;
        let gain = if c.gain == 0.0 { 1.0 } else { c.gain };
        let imbalance = if c.imbalance == 0.0 { 1.0 } else { c.imbalance };
        for frame in stereo.chunks_exact_mut(2) {
            let mut l = frame[0] as f64 * gain;
            let mut r = frame[1] as f64 * gain * imbalance;
            if c.noise > 0.0 {
                l += c.noise * self.rng.gauss() * 0.4;
                r += c.noise * self.rng.gauss() * 0.4;
            }
            if c.crackles_per_s > 0.0 {
                if self.crackle_left == 0 && self.rng.uniform() < c.crackles_per_s / self.sample_rate {
                    self.crackle_left = 6 + (self.rng.uniform() * 20.0) as usize;
                    self.crackle_amplitude = 0.3 + self.rng.uniform() * 0.6;
                }
                if self.crackle_left > 0 {
                    let v = self.crackle_amplitude * (self.rng.uniform() * 2.0 - 1.0);
                    l += v;
                    r += v;
                    self.crackle_left -= 1;
                }
            }
            if c.dropouts_per_s > 0.0 {
                if self.dropout_left == 0 && self.rng.uniform() < c.dropouts_per_s / self.sample_rate {
                    self.dropout_left = (c.dropout_ms / 1000.0 * self.sample_rate) as usize;
                }
                if self.dropout_left > 0 {
                    l *= 0.02;
                    r *= 0.02;
                    self.dropout_left -= 1;
                }
            }
            frame[0] = l.clamp(-1.0, 1.0) as f32;
            frame[1] = r.clamp(-1.0, 1.0) as f32;
        }
    }
}

/// Hand movement patterns for [`SimulatedTurntable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    /// Plays at +2%.
    Steady,
    /// Plays for 4 s, then scratches back and forth for 2 s.
    Scratch,
    /// Plays with short pitch nudges, like beatmatching by hand.
    Nudge,
}

impl Pattern {
    /// Record speed at time `t` (seconds).
    pub fn speed_at(self, t: f64) -> f64 {
        match self {
            Pattern::Steady => 1.02,
            Pattern::Nudge => {
                let c = t % 4.0;
                1.0 + if c < 0.3 {
                    0.06
                } else if (2.0..2.3).contains(&c) {
                    -0.06
                } else {
                    0.0
                }
            }
            Pattern::Scratch => {
                let c = t % 6.0;
                if c < 4.0 {
                    1.0
                } else {
                    -1.8 * (TAU * 2.0 * (c - 4.0)).sin()
                }
            }
        }
    }
}

/// A turntable played by a simulated hand: renders the signal the sound card would
/// receive. Lets the whole application run without hardware.
pub struct SimulatedTurntable {
    generator: SignalGenerator,
    pattern: Pattern,
    sample_rate: f64,
    t: f64,
    buffer: Vec<f32>,
}

impl SimulatedTurntable {
    /// Where the needle starts on the record (seconds).
    pub const START_S: f64 = 10.0;

    pub fn new(format: &'static TimecodeFormat, sample_rate: u32, pattern: Pattern) -> Self {
        Self {
            generator: SignalGenerator::new(format, sample_rate, Self::START_S * 1000.0, 900_000.0),
            pattern,
            sample_rate: sample_rate as f64,
            t: 0.0,
            buffer: Vec::with_capacity(16_384 * 2),
        }
    }

    /// Renders `frames` stereo frames (no allocation once the buffer has grown).
    pub fn render(&mut self, frames: usize) -> &[f32] {
        self.buffer.clear();
        let (t0, sr, pattern) = (self.t, self.sample_rate, self.pattern);
        self.generator.render(frames, |i| pattern.speed_at(t0 + i as f64 / sr), &mut self.buffer);
        self.t += frames as f64 / sr;
        &self.buffer
    }
}
