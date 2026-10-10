//! Mixing decks into the main output: channel volume, crossfader, master gain.

use crate::command::CrossfaderSide;
use std::f32::consts::FRAC_PI_2;

pub(crate) struct Mixer {
    /// -1.0 = full left, 1.0 = full right.
    pub(crate) crossfader: f32,
    pub(crate) master_gain: f32,
}

impl Mixer {
    pub(crate) fn new() -> Self {
        Self { crossfader: 0.0, master_gain: 1.0 }
    }

    /// Gain of the crossfader for a deck on `side` (equal-power curve).
    pub(crate) fn crossfader_gain(&self, side: CrossfaderSide) -> f32 {
        let x = (self.crossfader.clamp(-1.0, 1.0) + 1.0) * 0.5; // 0 = left, 1 = right
        match side {
            CrossfaderSide::Center => 1.0,
            CrossfaderSide::Left => (x * FRAC_PI_2).cos(),
            CrossfaderSide::Right => (x * FRAC_PI_2).sin(),
        }
    }

    /// Adds a deck's stereo block to the interleaved output (main = first two channels).
    pub(crate) fn add_deck(&self, deck_audio: &[f32], gain: f32, out: &mut [f32], out_channels: usize) {
        let g = gain * self.master_gain;
        for (frame, src) in out.chunks_exact_mut(out_channels).zip(deck_audio.chunks_exact(2)) {
            frame[0] += src[0] * g;
            if out_channels > 1 {
                frame[1] += src[1] * g;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossfader_is_equal_power() {
        let mut m = Mixer::new();
        m.crossfader = -1.0;
        assert!((m.crossfader_gain(CrossfaderSide::Left) - 1.0).abs() < 1e-6);
        assert!(m.crossfader_gain(CrossfaderSide::Right).abs() < 1e-6);
        m.crossfader = 0.0;
        let (l, r) = (m.crossfader_gain(CrossfaderSide::Left), m.crossfader_gain(CrossfaderSide::Right));
        assert!((l * l + r * r - 1.0).abs() < 1e-5);
        assert_eq!(m.crossfader_gain(CrossfaderSide::Center), 1.0);
    }
}
