//! Beat effects: one effect at a time on a deck, timed on its tempo.
//!
//! Two kinds of effects:
//! * sent (echo, ping-pong, reverb): the dry signal stays whole and the effect is added
//!   to it; switched off, the effect stops receiving and its tail rings out;
//! * inserted (flanger, phaser, filter, gate, bitcrusher): the signal crossfades to its
//!   processed version, and back when switched off.
//!
//! Every effect's memory is allocated when the slot is built: nothing is allocated on
//! the audio thread, even when switching effects.

mod delay;
mod echo;
mod gate;
mod modulation;
mod reverb;

use crate::smooth::Smoothed;
use echo::{Echo, PingPong};
use gate::{Bitcrusher, Gate};
use modulation::{AutoFilter, Flanger, Phaser};
use reverb::Reverb;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EffectKind {
    #[default]
    Echo,
    PingPong,
    Reverb,
    Flanger,
    Phaser,
    Filter,
    Gate,
    Bitcrusher,
}

impl EffectKind {
    fn is_sent(self) -> bool {
        matches!(self, EffectKind::Echo | EffectKind::PingPong | EffectKind::Reverb)
    }
}

/// The beat an effect follows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Beat {
    /// Length of a beat (seconds).
    pub seconds: f32,
    /// Where the block starts within the beat (0..1), when the deck has a beat grid.
    pub phase: Option<f32>,
}

impl Beat {
    /// Without a beat grid: 120 BPM, free running.
    pub const DEFAULT: Beat = Beat { seconds: 0.5, phase: None };
}

/// Shortest and longest effect period, in beats.
pub const MIN_BEATS: f32 = 1.0 / 8.0;
pub const MAX_BEATS: f32 = 8.0;
const GLIDE_S: f32 = 0.02;
/// Below this level the tail of a switched-off effect is over.
const SILENT: f32 = 1e-4;

/// What an effect needs for one block.
pub(crate) struct Params {
    /// The effect's main knob, 0..1.
    pub amount: f32,
    /// The effect's period: delay, modulation cycle, gate length (seconds).
    pub period_s: f32,
    /// Position within the beat at the start of the block, when known.
    pub phase: Option<f32>,
    pub beat_s: f32,
    pub sample_rate: f32,
}

/// One deck's effect.
pub struct FxSlot {
    sample_rate: f32,
    kind: EffectKind,
    enabled: bool,
    /// How much of the signal goes to the effect: fades with ``enabled``.
    send: Smoothed,
    mix: Smoothed,
    amount: f32,
    beats: f32,
    /// Something is still coming out of the effect.
    sounding: bool,
    sends: Vec<f32>,
    wet: Vec<f32>,
    echo: Echo,
    ping_pong: PingPong,
    reverb: Reverb,
    flanger: Flanger,
    phaser: Phaser,
    filter: AutoFilter,
    gate: Gate,
    bitcrusher: Bitcrusher,
}

impl FxSlot {
    pub fn new(sample_rate: f32, max_block_frames: usize) -> Self {
        Self {
            sample_rate,
            kind: EffectKind::Echo,
            enabled: false,
            send: Smoothed::new(0.0, GLIDE_S, sample_rate),
            mix: Smoothed::new(0.5, GLIDE_S, sample_rate),
            amount: 0.5,
            beats: 1.0,
            sounding: false,
            sends: vec![0.0; max_block_frames],
            wet: vec![0.0; max_block_frames * 2],
            echo: Echo::new(sample_rate),
            ping_pong: PingPong::new(sample_rate),
            reverb: Reverb::new(sample_rate),
            flanger: Flanger::new(sample_rate),
            phaser: Phaser::default(),
            filter: AutoFilter::default(),
            gate: Gate::default(),
            bitcrusher: Bitcrusher::default(),
        }
    }

    pub fn kind(&self) -> EffectKind {
        self.kind
    }

    /// Changes the effect; the new one starts from silence.
    pub fn set_kind(&mut self, kind: EffectKind) {
        if kind != self.kind {
            self.kind = kind;
            self.reset_effect();
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.send.set(if enabled { 1.0 } else { 0.0 });
        self.sounding |= enabled;
    }

    /// Level of the effect, 0..1.
    pub fn set_mix(&mut self, mix: f32) {
        self.mix.set(mix.clamp(0.0, 1.0));
    }

    /// The effect's main knob (feedback, depth...), 0..1.
    pub fn set_amount(&mut self, amount: f32) {
        self.amount = amount.clamp(0.0, 1.0);
    }

    /// The effect's period in beats, [``MIN_BEATS``]..[``MAX_BEATS``].
    pub fn set_beats(&mut self, beats: f32) {
        self.beats = beats.clamp(MIN_BEATS, MAX_BEATS);
    }

    /// Applies the effect to interleaved stereo, in place.
    pub fn process(&mut self, stereo: &mut [f32], beat: Beat) {
        if !self.sounding {
            return;
        }
        let frames = stereo.len() / 2;
        let sent = self.kind.is_sent();
        let sends = &mut self.sends[..frames];
        let wet = &mut self.wet[..stereo.len()];
        for ((send, w), dry) in sends.iter_mut().zip(wet.chunks_exact_mut(2)).zip(stereo.chunks_exact(2)) {
            *send = self.send.tick();
            let input = if sent { *send } else { 1.0 };
            w[0] = dry[0] * input;
            w[1] = dry[1] * input;
        }
        let params = Params {
            amount: self.amount,
            period_s: self.beats * beat.seconds,
            phase: beat.phase,
            beat_s: beat.seconds,
            sample_rate: self.sample_rate,
        };
        match self.kind {
            EffectKind::Echo => self.echo.process(wet, &params),
            EffectKind::PingPong => self.ping_pong.process(wet, &params),
            EffectKind::Reverb => self.reverb.process(wet, &params),
            EffectKind::Flanger => self.flanger.process(wet, &params),
            EffectKind::Phaser => self.phaser.process(wet, &params),
            EffectKind::Filter => self.filter.process(wet, &params),
            EffectKind::Gate => self.gate.process(wet, &params),
            EffectKind::Bitcrusher => self.bitcrusher.process(wet, &params),
        }
        let mut loudest = 0.0f32;
        for ((s, w), send) in stereo.chunks_exact_mut(2).zip(wet.chunks_exact(2)).zip(sends.iter()) {
            let mix = self.mix.tick();
            if sent {
                s[0] += w[0] * mix;
                s[1] += w[1] * mix;
                loudest = loudest.max(w[0].abs()).max(w[1].abs());
            } else {
                let m = mix * send;
                s[0] += (w[0] - s[0]) * m;
                s[1] += (w[1] - s[1]) * m;
            }
        }
        let faded_out = !self.enabled && self.send.is_settled();
        if faded_out && (!sent || loudest < SILENT) {
            self.reset_effect();
        }
    }

    fn reset_effect(&mut self) {
        self.sounding = self.enabled;
        self.echo.reset();
        self.ping_pong.reset();
        self.reverb.reset();
        self.flanger.reset();
        self.phaser = Phaser::default();
        self.filter = AutoFilter::default();
        self.gate = Gate::default();
        self.bitcrusher = Bitcrusher::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    fn slot(kind: EffectKind) -> FxSlot {
        let mut slot = FxSlot::new(RATE, 256);
        slot.set_kind(kind);
        slot.set_mix(1.0);
        slot.set_enabled(true);
        slot
    }

    /// Runs `input` (left channel, both sides the same) through the slot.
    fn run(slot: &mut FxSlot, input: &[f32]) -> Vec<f32> {
        let mut stereo: Vec<f32> = input.iter().flat_map(|&v| [v, v]).collect();
        for block in stereo.chunks_mut(256) {
            slot.process(block, Beat::DEFAULT);
        }
        stereo.chunks_exact(2).map(|f| f[0]).collect()
    }

    fn impulse(seconds: f32) -> Vec<f32> {
        let mut signal = vec![0.0; (seconds * RATE) as usize];
        signal[100] = 1.0;
        signal
    }

    #[test]
    fn echo_repeats_on_the_beat_and_rings_out() {
        let mut echo = slot(EffectKind::Echo);
        echo.set_beats(0.5);
        let out = run(&mut echo, &impulse(1.0));
        let repeat = 100 + (0.25 * RATE) as usize;
        assert!(out[repeat - 3..repeat + 3].iter().any(|v| v.abs() > 0.2), "a repeat a half beat later");
        assert!(out[1_000..repeat - 100].iter().all(|v| v.abs() < 1e-3), "silence in between");

        echo.set_enabled(false);
        let tail = run(&mut echo, &vec![0.0; RATE as usize]);
        assert!(tail.iter().any(|v| v.abs() > 0.01), "switched off, it still rings");
    }

    #[test]
    fn reverb_leaves_a_tail() {
        let mut reverb = slot(EffectKind::Reverb);
        let out = run(&mut reverb, &impulse(0.5));
        let late: f32 = out[(0.2 * RATE) as usize..].iter().map(|v| v.abs()).sum();
        assert!(late > 0.1, "{late}");
    }

    #[test]
    fn gate_chops() {
        let mut gate = slot(EffectKind::Gate);
        gate.set_amount(1.0);
        let out = run(&mut gate, &vec![0.5; RATE as usize]);
        let quiet = out[(0.1 * RATE) as usize..].iter().filter(|v| v.abs() < 0.01).count();
        assert!(quiet > (0.3 * RATE) as usize, "closed about half the time: {quiet}");
    }

    #[test]
    fn inserted_effects_go_away_when_switched_off() {
        let mut crusher = slot(EffectKind::Bitcrusher);
        crusher.set_amount(1.0);
        let input: Vec<f32> = (0..RATE as usize).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let crushed = run(&mut crusher, &input);
        assert!(crushed.iter().zip(&input).any(|(c, i)| (c - i).abs() > 0.05));
        crusher.set_enabled(false);
        run(&mut crusher, &input);
        let clean = run(&mut crusher, &input);
        assert!(clean.iter().zip(&input).all(|(c, i)| (c - i).abs() < 1e-6));
    }

    #[test]
    fn every_effect_runs() {
        for kind in [
            EffectKind::Echo,
            EffectKind::PingPong,
            EffectKind::Reverb,
            EffectKind::Flanger,
            EffectKind::Phaser,
            EffectKind::Filter,
            EffectKind::Gate,
            EffectKind::Bitcrusher,
        ] {
            let mut fx = slot(kind);
            let input: Vec<f32> = (0..RATE as usize).map(|i| (i as f32 * 0.02).sin() * 0.5).collect();
            let out = run(&mut fx, &input);
            assert!(out.iter().all(|v| v.is_finite() && v.abs() < 4.0), "{kind:?}");
            assert!(out.iter().zip(&input).any(|(o, i)| (o - i).abs() > 1e-3), "{kind:?} does something");
        }
    }
}
