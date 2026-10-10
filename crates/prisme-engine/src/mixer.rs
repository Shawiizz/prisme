//! Where the decks go: each on its own outputs to an external mixer, or through the
//! software mixer (channel volumes, crossfader, master with its limiter, headphones).

use crate::command::CrossfaderSide;
use crate::status::MAX_DECKS;
use prisme_dsp::{DelayLine, Limiter};
use std::f32::consts::FRAC_PI_2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixMode {
    /// Each deck on its own output pair, for a hardware mixer: no software volume,
    /// crossfader or headphones.
    External,
    /// The software mixer: master and headphones.
    Internal,
}

/// First output channel of each stereo pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputRouting {
    pub mode: MixMode,
    /// External mode: each deck's pair.
    pub deck_outputs: [Option<usize>; MAX_DECKS],
    /// Internal mode.
    pub master: Option<usize>,
    pub headphones: Option<usize>,
}

impl Default for OutputRouting {
    /// The software mixer, which works on any device; decks on outputs 1-2, 3-4... in
    /// external mode, master on 1-2 and headphones on 3-4.
    fn default() -> Self {
        Self {
            mode: MixMode::Internal,
            deck_outputs: [Some(0), Some(2), Some(4), Some(6)],
            master: Some(0),
            headphones: Some(2),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CrossfaderCurve {
    /// Equal power: a smooth blend with no dip in the middle.
    #[default]
    Smooth,
    /// Both decks at full volume except at the very ends, for cutting while scratching.
    Sharp,
}

/// Sharp curve: the travel at each end over which a deck is cut.
const SHARP_CUT: f32 = 0.04;
/// Highest master level: just below full scale.
const LIMITER_THRESHOLD: f32 = 0.98;

pub(crate) struct Mixer {
    pub(crate) routing: OutputRouting,
    /// -1.0 = full left, 1.0 = full right.
    pub(crate) crossfader: f32,
    pub(crate) curve: CrossfaderCurve,
    pub(crate) limiter: Limiter,
    /// Keeps the headphone cue in time with the master, which the limiter delays.
    cue_delay: DelayLine,
    pub(crate) master_gain: f32,
    /// Headphones: 0 = cued decks only, 1 = master only.
    pub(crate) cue_mix: f32,
    pub(crate) headphone_gain: f32,
    master_bus: Vec<f32>,
    cue_bus: Vec<f32>,
}

/// What the mixer needs to know about a deck for one block.
pub(crate) struct DeckMix<'a> {
    pub(crate) audio: &'a [f32],
    pub(crate) volume: f32,
    pub(crate) side: CrossfaderSide,
    pub(crate) cue: bool,
}

impl Mixer {
    pub(crate) fn new(max_block_frames: usize, sample_rate: f32) -> Self {
        let limiter = Limiter::new(sample_rate, LIMITER_THRESHOLD);
        Self {
            routing: OutputRouting::default(),
            crossfader: 0.0,
            curve: CrossfaderCurve::Smooth,
            cue_delay: DelayLine::new(limiter.latency()),
            limiter,
            master_gain: 1.0,
            cue_mix: 0.0,
            headphone_gain: 1.0,
            master_bus: vec![0.0; max_block_frames * 2],
            cue_bus: vec![0.0; max_block_frames * 2],
        }
    }

    pub(crate) fn begin(&mut self, frames: usize) {
        self.master_bus[..frames * 2].fill(0.0);
        self.cue_bus[..frames * 2].fill(0.0);
    }

    /// Adds deck `index` (stereo, interleaved) to the outputs or the buses.
    pub(crate) fn add_deck(&mut self, index: usize, deck: DeckMix<'_>, out: &mut [f32], out_channels: usize) {
        let n = deck.audio.len();
        match self.routing.mode {
            MixMode::External => {
                if let Some(first) = self.routing.deck_outputs[index] {
                    add_pair(deck.audio, self.master_gain, out, out_channels, first);
                }
            }
            MixMode::Internal => {
                let gain = deck.volume * self.crossfader_gain(deck.side);
                accumulate(&mut self.master_bus[..n], deck.audio, gain);
                if deck.cue {
                    accumulate(&mut self.cue_bus[..n], deck.audio, 1.0);
                }
            }
        }
    }

    /// Writes the buses of the internal mixer to their outputs.
    pub(crate) fn finish(&mut self, frames: usize, out: &mut [f32], out_channels: usize) {
        if self.routing.mode != MixMode::Internal {
            return;
        }
        let master = &mut self.master_bus[..frames * 2];
        for s in master.iter_mut() {
            *s *= self.master_gain;
        }
        self.limiter.process(master);
        let master = &self.master_bus[..frames * 2];
        if let Some(first) = self.routing.master {
            add_pair(master, 1.0, out, out_channels, first);
        }
        if let Some(first) = self.routing.headphones {
            if self.limiter.is_enabled() {
                self.cue_delay.process(&mut self.cue_bus[..frames * 2]);
            }
            let mix = self.cue_mix.clamp(0.0, 1.0);
            add_pair(
                &self.cue_bus[..frames * 2],
                (1.0 - mix) * self.headphone_gain,
                out,
                out_channels,
                first,
            );
            add_pair(master, mix * self.headphone_gain, out, out_channels, first);
        }
    }

    /// The master of the software mixer for the block just finished (silence in
    /// external mode).
    pub(crate) fn master(&self, frames: usize) -> &[f32] {
        &self.master_bus[..frames * 2]
    }

    /// Gain of the crossfader for a deck on `side`.
    pub(crate) fn crossfader_gain(&self, side: CrossfaderSide) -> f32 {
        // how far the crossfader is towards the other side, 0..1
        let x = (self.crossfader.clamp(-1.0, 1.0) + 1.0) * 0.5;
        let away = match side {
            CrossfaderSide::Center => return 1.0,
            CrossfaderSide::Left => x,
            CrossfaderSide::Right => 1.0 - x,
        };
        match self.curve {
            CrossfaderCurve::Smooth => (away * FRAC_PI_2).cos(),
            CrossfaderCurve::Sharp => ((1.0 - away) / SHARP_CUT).min(1.0),
        }
    }
}

fn accumulate(bus: &mut [f32], audio: &[f32], gain: f32) {
    for (b, s) in bus.iter_mut().zip(audio) {
        *b += s * gain;
    }
}

/// Adds a stereo block to the pair starting at channel `first`; a pair cut short by the
/// device's channel count gets the mono sum on its one channel.
fn add_pair(stereo: &[f32], gain: f32, out: &mut [f32], out_channels: usize, first: usize) {
    if first >= out_channels || gain == 0.0 {
        return;
    }
    for (frame, src) in out.chunks_exact_mut(out_channels).zip(stereo.chunks_exact(2)) {
        if first + 1 < out_channels {
            frame[first] += src[0] * gain;
            frame[first + 1] += src[1] * gain;
        } else {
            frame[first] += (src[0] + src[1]) * 0.5 * gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deck(audio: &[f32], side: CrossfaderSide, cue: bool) -> DeckMix<'_> {
        DeckMix { audio, volume: 1.0, side, cue }
    }

    #[test]
    fn crossfader_is_equal_power() {
        let mut m = Mixer::new(4, 48_000.0);
        m.crossfader = -1.0;
        assert!((m.crossfader_gain(CrossfaderSide::Left) - 1.0).abs() < 1e-6);
        assert!(m.crossfader_gain(CrossfaderSide::Right).abs() < 1e-6);
        m.crossfader = 0.0;
        let (l, r) = (m.crossfader_gain(CrossfaderSide::Left), m.crossfader_gain(CrossfaderSide::Right));
        assert!((l * l + r * r - 1.0).abs() < 1e-5);
        assert_eq!(m.crossfader_gain(CrossfaderSide::Center), 1.0);
    }

    #[test]
    fn sharp_crossfader_cuts_only_at_the_ends() {
        let mut m = Mixer::new(4, 48_000.0);
        m.curve = CrossfaderCurve::Sharp;
        m.crossfader = 0.0;
        assert_eq!(
            (m.crossfader_gain(CrossfaderSide::Left), m.crossfader_gain(CrossfaderSide::Right)),
            (1.0, 1.0)
        );
        m.crossfader = 0.9;
        assert_eq!(m.crossfader_gain(CrossfaderSide::Left), 1.0, "still in");
        m.crossfader = 1.0;
        assert_eq!(m.crossfader_gain(CrossfaderSide::Left), 0.0, "cut");
    }

    #[test]
    fn external_mode_sends_each_deck_to_its_own_outputs() {
        let mut m = Mixer::new(1, 48_000.0);
        m.routing.mode = MixMode::External;
        m.crossfader = -1.0;
        let mut out = [0.0f32; 4];
        m.begin(1);
        m.add_deck(0, deck(&[0.1, 0.2], CrossfaderSide::Left, false), &mut out, 4);
        m.add_deck(1, deck(&[0.3, 0.4], CrossfaderSide::Right, false), &mut out, 4);
        m.finish(1, &mut out, 4);
        assert_eq!(out, [0.1, 0.2, 0.3, 0.4], "no crossfader in external mode");
    }

    #[test]
    fn internal_mode_mixes_the_master_and_cues_the_headphones() {
        let mut m = Mixer::new(1, 48_000.0);
        m.limiter.set_enabled(false);
        m.crossfader = -1.0;
        let mut out = [0.0f32; 4];
        m.begin(1);
        m.add_deck(0, deck(&[0.1, 0.1], CrossfaderSide::Left, false), &mut out, 4);
        m.add_deck(1, deck(&[0.3, 0.3], CrossfaderSide::Right, true), &mut out, 4);
        m.finish(1, &mut out, 4);
        assert!((out[0] - 0.1).abs() < 1e-6, "deck 2 is faded out of the master");
        assert!((out[2] - 0.3).abs() < 1e-6, "but heard in the headphones");

        m.cue_mix = 1.0;
        let mut out = [0.0f32; 4];
        m.begin(1);
        m.add_deck(0, deck(&[0.1, 0.1], CrossfaderSide::Left, false), &mut out, 4);
        m.add_deck(1, deck(&[0.3, 0.3], CrossfaderSide::Right, true), &mut out, 4);
        m.finish(1, &mut out, 4);
        assert!((out[2] - 0.1).abs() < 1e-6, "headphones on the master");
    }

    #[test]
    fn the_cue_stays_in_time_with_the_limited_master() {
        let mut m = Mixer::new(64, 48_000.0);
        m.cue_mix = 0.5;
        let mut impulse = [0.0f32; 128];
        impulse[0] = 0.5;
        impulse[1] = 0.5;
        let mut out = vec![0.0f32; 64 * 4];
        let mut heard = Vec::new();
        for block in 0..4 {
            out.fill(0.0);
            m.begin(64);
            let audio = if block == 0 { impulse } else { [0.0; 128] };
            m.add_deck(0, deck(&audio, CrossfaderSide::Center, true), &mut out, 4);
            m.finish(64, &mut out, 4);
            heard.extend(out.chunks_exact(4).map(|f| (f[0], f[2])));
        }
        let master_at = heard.iter().position(|(master, _)| *master > 0.1).unwrap();
        let phones_at = heard.iter().position(|(_, phones)| *phones > 0.1).unwrap();
        assert_eq!(master_at, m.limiter.latency());
        assert_eq!(phones_at, master_at, "one click in the headphones, not two");
        assert!((heard[phones_at].1 - 0.5).abs() < 1e-6, "cue and master add up");
    }

    #[test]
    fn a_pair_beyond_the_device_channels_is_skipped_or_summed() {
        let mut out = [0.0f32; 3];
        add_pair(&[0.2, 0.4], 1.0, &mut out, 3, 2);
        assert!((out[2] - 0.3).abs() < 1e-6);
        let mut out = [0.0f32; 2];
        add_pair(&[0.2, 0.4], 1.0, &mut out, 2, 2);
        assert_eq!(out, [0.0, 0.0]);
    }
}
