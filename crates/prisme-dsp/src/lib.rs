//! Real-time audio processing for the engine: every type here processes in place,
//! without allocating, locking or freeing once built, and changes its parameters
//! without clicks.
//!
//! * [`svf`]: the state-variable filter everything else is built from;
//! * [`eq`]: the 3-band isolator EQ of a mixer channel;
//! * [`dj_filter`]: the one-knob low-pass / high-pass filter;
//! * [`fx`]: beat effects (echo, reverb, flanger, gate...);
//! * [`limiter`]: the master limiter, and [`delay`] to keep other signals in time with it;
//! * [`smooth`]: parameter smoothing;
//! * [`denormal`]: keeps the arithmetic fast near silence (to hold during processing).

pub mod delay;
pub mod denormal;
pub mod dj_filter;
pub mod eq;
pub mod fx;
pub mod limiter;
pub mod smooth;
pub mod svf;

pub use delay::DelayLine;
pub use denormal::DenormalGuard;
pub use dj_filter::DjFilter;
pub use eq::{Band, IsolatorEq};
pub use fx::{Beat, EffectKind, FxSlot};
pub use limiter::Limiter;
pub use smooth::Smoothed;

/// Decibels to a linear gain.
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// A linear gain in decibels (-inf for 0).
pub fn gain_to_db(gain: f32) -> f32 {
    20.0 * gain.log10()
}

#[cfg(test)]
pub(crate) mod testing {
    use std::f64::consts::TAU;

    /// Interleaved stereo sine, the same on both channels.
    pub fn sine(freq: f64, rate: f64, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let v = (TAU * freq * i as f64 / rate).sin() as f32;
                [v, v]
            })
            .collect()
    }

    /// RMS of the left channel, skipping the first `skip` frames (filters settling).
    pub fn rms(stereo: &[f32], skip: usize) -> f64 {
        let left: Vec<f64> = stereo.chunks_exact(2).skip(skip).map(|f| f[0] as f64).collect();
        (left.iter().map(|v| v * v).sum::<f64>() / left.len() as f64).sqrt()
    }

    /// Gain of a processor for a sine at `freq`, in dB.
    pub fn response_db(freq: f64, rate: f64, mut process: impl FnMut(&mut [f32])) -> f64 {
        let frames = rate as usize;
        let mut signal = sine(freq, rate, frames);
        let before = rms(&signal, frames / 2);
        for block in signal.chunks_mut(256) {
            process(block);
        }
        20.0 * (rms(&signal, frames / 2) / before).log10()
    }
}
