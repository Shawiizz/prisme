//! A channel of the software mixer, before its fader: trim, 3-band EQ with kills, DJ
//! filter.

use prisme_dsp::{Band, DjFilter, IsolatorEq, Smoothed};

const TRIM_GLIDE_S: f32 = 0.01;

pub(crate) struct ChannelStrip {
    trim: Smoothed,
    eq: IsolatorEq,
    /// Gains set by the EQ knobs; a killed band is at 0 whatever its knob.
    gains: [f32; 3],
    kills: [bool; 3],
    filter: DjFilter,
}

impl ChannelStrip {
    pub(crate) fn new(sample_rate: f32) -> Self {
        Self {
            trim: Smoothed::new(1.0, TRIM_GLIDE_S, sample_rate),
            eq: IsolatorEq::new(sample_rate),
            gains: [1.0; 3],
            kills: [false; 3],
            filter: DjFilter::new(sample_rate),
        }
    }

    pub(crate) fn set_trim(&mut self, gain: f32) {
        self.trim.set(gain.max(0.0));
    }

    pub(crate) fn set_eq(&mut self, band: Band, gain: f32) {
        self.gains[band as usize] = gain.max(0.0);
        self.apply_eq(band);
    }

    pub(crate) fn set_kill(&mut self, band: Band, kill: bool) {
        self.kills[band as usize] = kill;
        self.apply_eq(band);
    }

    pub(crate) fn set_filter(&mut self, position: f32) {
        self.filter.set_position(position);
    }

    fn apply_eq(&mut self, band: Band) {
        let i = band as usize;
        self.eq.set_gain(band, if self.kills[i] { 0.0 } else { self.gains[i] });
    }

    pub(crate) fn process(&mut self, stereo: &mut [f32]) {
        if !(self.trim.is_settled() && self.trim.target() == 1.0) {
            for frame in stereo.chunks_exact_mut(2) {
                let gain = self.trim.tick();
                frame[0] *= gain;
                frame[1] *= gain;
            }
        }
        self.eq.process(stereo);
        self.filter.process(stereo);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(strip: &mut ChannelStrip, freq: f32) -> f32 {
        let mut audio: Vec<f32> = (0..48_000)
            .flat_map(|i| {
                let v = (std::f32::consts::TAU * freq * i as f32 / 48_000.0).sin();
                [v, v]
            })
            .collect();
        for block in audio.chunks_mut(256) {
            strip.process(block);
        }
        audio[48_000..].iter().fold(0.0, |m, s| m.max(s.abs()))
    }

    #[test]
    fn a_kill_overrides_the_knob_and_lets_it_back() {
        let mut strip = ChannelStrip::new(48_000.0);
        strip.set_eq(Band::Low, 1.5);
        strip.set_kill(Band::Low, true);
        assert!(level(&mut strip, 50.0) < 0.02);
        strip.set_kill(Band::Low, false);
        assert!((level(&mut strip, 50.0) - 1.5).abs() < 0.05);
    }

    #[test]
    fn trim() {
        let mut strip = ChannelStrip::new(48_000.0);
        strip.set_trim(0.5);
        assert!((level(&mut strip, 1_000.0) - 0.5).abs() < 0.02);
    }
}
