//! Beat effects timed on a deck's tempo: a chain of slots in series, one effect in each.
//!
//! Two kinds of effects:
//! * sent (echo, ping-pong, reverb): the dry signal stays whole and the effect is added
//!   to it; switched off, the effect stops receiving and its tail rings out;
//! * inserted (flanger, phaser, filter, gate, bitcrusher): the signal crossfades to its
//!   processed version, and back when switched off.
//!
//! Every effect's memory is allocated when the chain is built: nothing is allocated on
//! the audio thread, even when switching effects, and a slot left off costs nothing.

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

/// Where a second knob starts: the effect as it sounded before it had one.
pub const PARAM2_DEFAULT: f32 = 0.5;

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

/// Slots in a deck's chain.
pub const FX_SLOTS: usize = 3;
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
    /// The effect's second knob (tone, resonance, width...), 0..1: in the middle, the effect
    /// as it sounds without one.
    pub param2: f32,
    /// The effect's period: delay, modulation cycle, gate length (seconds).
    pub period_s: f32,
    /// Position within the beat at the start of the block, when known.
    pub phase: Option<f32>,
    pub beat_s: f32,
    pub sample_rate: f32,
}

/// One effect of a deck's chain.
pub struct FxSlot {
    sample_rate: f32,
    kind: EffectKind,
    enabled: bool,
    /// Its chain is switched off: the slot is silent, its own switch kept.
    bypassed: bool,
    /// How much of the signal goes to the effect: fades in and out as the slot sounds or not.
    send: Smoothed,
    mix: Smoothed,
    amount: f32,
    param2: f32,
    beats: f32,
    /// Something is still coming out of the effect.
    sounding: bool,
    /// Frames a sent effect has been silent for: a delay is silent between its repeats.
    quiet_frames: usize,
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
            bypassed: false,
            send: Smoothed::new(0.0, GLIDE_S, sample_rate),
            mix: Smoothed::new(0.5, GLIDE_S, sample_rate),
            amount: 0.5,
            param2: PARAM2_DEFAULT,
            beats: 1.0,
            sounding: false,
            quiet_frames: 0,
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
        self.switch();
    }

    /// Silences the slot with its chain, or lets it sound again if it is on; a tail rings
    /// out as when the slot is switched off.
    fn set_bypassed(&mut self, bypassed: bool) {
        self.bypassed = bypassed;
        self.switch();
    }

    fn sounds(&self) -> bool {
        self.enabled && !self.bypassed
    }

    fn switch(&mut self) {
        let on = self.sounds();
        self.send.set(if on { 1.0 } else { 0.0 });
        self.sounding |= on;
    }

    /// Level of the effect, 0..1.
    pub fn set_mix(&mut self, mix: f32) {
        self.mix.set(mix.clamp(0.0, 1.0));
    }

    /// The effect's main knob (feedback, depth...), 0..1.
    pub fn set_amount(&mut self, amount: f32) {
        self.amount = amount.clamp(0.0, 1.0);
    }

    /// The effect's second knob (tone, damping, width, depth, resonance, length, downsampling:
    /// see each effect), 0..1.
    pub fn set_param2(&mut self, param2: f32) {
        self.param2 = param2.clamp(0.0, 1.0);
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
            param2: self.param2,
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
        self.quiet_frames = if loudest < SILENT { self.quiet_frames.saturating_add(frames) } else { 0 };
        // silent for a whole period, nothing is left in a delay to repeat
        let tail_over = self.quiet_frames as f32 > params.period_s * self.sample_rate;
        let faded_out = !self.sounds() && self.send.is_settled();
        if faded_out && (!sent || tail_over) {
            self.reset_effect();
        }
    }

    fn reset_effect(&mut self) {
        self.sounding = self.sounds();
        self.quiet_frames = 0;
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

/// A deck's effects, one slot after the other: each takes what the one before gives.
pub struct FxChain {
    slots: [FxSlot; FX_SLOTS],
    enabled: bool,
}

impl FxChain {
    pub fn new(sample_rate: f32, max_block_frames: usize) -> Self {
        Self { slots: std::array::from_fn(|_| FxSlot::new(sample_rate, max_block_frames)), enabled: true }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// The whole chain on, or off: each slot keeps its own switch, heard again with the chain.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        for slot in &mut self.slots {
            slot.set_bypassed(!enabled);
        }
    }

    /// The slot at `index`, if the chain has it.
    pub fn slot_mut(&mut self, index: usize) -> Option<&mut FxSlot> {
        self.slots.get_mut(index)
    }

    /// Applies the slots in order to interleaved stereo, in place.
    pub fn process(&mut self, stereo: &mut [f32], beat: Beat) {
        for slot in &mut self.slots {
            slot.process(stereo, beat);
        }
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
    fn a_chain_left_off_changes_nothing() {
        let mut chain = FxChain::new(RATE, 256);
        let input: Vec<f32> = (0..4096).map(|i| (i as f32 * 0.03).sin() * 0.5).collect();
        let mut stereo = input.clone();
        for block in stereo.chunks_mut(512) {
            chain.process(block, Beat::DEFAULT);
        }
        assert_eq!(stereo, input);
    }

    #[test]
    fn a_chain_runs_its_slots_one_after_the_other() {
        let setups = [(EffectKind::Echo, 0.5), (EffectKind::Gate, 1.0), (EffectKind::Bitcrusher, 0.25)];
        let mut chain = FxChain::new(RATE, 256);
        let mut alone: Vec<FxSlot> = setups.iter().map(|&(kind, _)| slot(kind)).collect();
        for (i, &(kind, beats)) in setups.iter().enumerate() {
            let in_chain = chain.slot_mut(i).unwrap();
            in_chain.set_kind(kind);
            in_chain.set_mix(1.0);
            in_chain.set_enabled(true);
            in_chain.set_beats(beats);
            alone[i].set_beats(beats);
        }
        assert!(chain.slot_mut(FX_SLOTS).is_none());
        let input: Vec<f32> = (0..RATE as usize).map(|i| (i as f32 * 0.02).sin() * 0.5).collect();
        let mut chained: Vec<f32> = input.iter().flat_map(|&v| [v, v]).collect();
        for block in chained.chunks_mut(512) {
            chain.process(block, Beat::DEFAULT);
        }
        let in_turn = alone.iter_mut().fold(input, |signal, slot| run(slot, &signal));
        let chained: Vec<f32> = chained.chunks_exact(2).map(|f| f[0]).collect();
        assert_eq!(chained, in_turn);
    }

    const KINDS: [EffectKind; 8] = [
        EffectKind::Echo,
        EffectKind::PingPong,
        EffectKind::Reverb,
        EffectKind::Flanger,
        EffectKind::Phaser,
        EffectKind::Filter,
        EffectKind::Gate,
        EffectKind::Bitcrusher,
    ];

    fn sine() -> Vec<f32> {
        (0..RATE as usize).map(|i| (i as f32 * 0.02).sin() * 0.5).collect()
    }

    #[test]
    fn every_effect_runs() {
        for kind in KINDS {
            let mut fx = slot(kind);
            let input = sine();
            let out = run(&mut fx, &input);
            assert!(out.iter().all(|v| v.is_finite() && v.abs() < 4.0), "{kind:?}");
            assert!(out.iter().zip(&input).any(|(o, i)| (o - i).abs() > 1e-3), "{kind:?} does something");
        }
    }

    #[test]
    fn every_second_knob_changes_its_effect_and_starts_where_it_was() {
        // a chord, so that tone and resonance have highs to work on; long enough for the
        // repeats of repeats
        let input: Vec<f32> = (0..2 * RATE as usize)
            .map(|i| ((i as f32 * 0.02).sin() + (i as f32 * 0.7).sin()) * 0.3)
            .collect();
        for kind in KINDS {
            let at = |param2: Option<f32>| {
                let mut fx = slot(kind);
                fx.set_amount(0.8);
                if let Some(param2) = param2 {
                    fx.set_param2(param2);
                }
                run(&mut fx, &input)
            };
            assert_eq!(at(Some(PARAM2_DEFAULT)), at(None), "{kind:?}");
            let (low, high) = (at(Some(0.0)), at(Some(1.0)));
            assert!(high.iter().chain(&low).all(|v| v.is_finite() && v.abs() < 4.0), "{kind:?}");
            let apart = low.iter().zip(&high).map(|(l, h)| (l - h).abs()).fold(0.0, f32::max);
            assert!(apart > 0.01, "{kind:?}: {apart}");
        }
    }

    #[test]
    fn the_second_knob_of_the_gate_is_how_long_it_stays_open() {
        let mut gate = slot(EffectKind::Gate);
        gate.set_amount(1.0);
        gate.set_param2(0.25);
        let out = run(&mut gate, &vec![0.5; 2 * RATE as usize]);
        let share = out.iter().filter(|v| v.abs() < 0.01).count() as f32 / out.len() as f32;
        assert!((0.7..0.8).contains(&share), "closed three quarters of the time: {share}");
    }

    #[test]
    fn a_chain_switched_off_lets_its_tails_ring_and_keeps_its_slots() {
        let mut chain = FxChain::new(RATE, 256);
        let echo = chain.slot_mut(0).unwrap();
        echo.set_mix(1.0);
        echo.set_enabled(true);
        let crusher = chain.slot_mut(1).unwrap();
        crusher.set_kind(EffectKind::Bitcrusher);
        crusher.set_amount(1.0);
        crusher.set_mix(1.0);
        crusher.set_enabled(true);
        let process = |chain: &mut FxChain, input: &[f32]| {
            let mut stereo: Vec<f32> = input.iter().flat_map(|&v| [v, v]).collect();
            for block in stereo.chunks_mut(256) {
                chain.process(block, Beat::DEFAULT);
            }
            stereo.chunks_exact(2).map(|f| f[0]).collect::<Vec<f32>>()
        };
        process(&mut chain, &sine()[..(0.25 * RATE) as usize]);

        chain.set_enabled(false);
        assert!(!chain.enabled());
        let tail = process(&mut chain, &vec![0.0; (0.6 * RATE) as usize]);
        assert!(tail.iter().any(|v| v.abs() > 0.01), "the echo rings out");
        process(&mut chain, &vec![0.0; 10 * RATE as usize]);
        let input = sine();
        assert_eq!(process(&mut chain, &input), input, "nothing sounds once the tails are over");
        assert!(chain.slots.iter().take(2).all(|s| s.enabled), "the slots stay on");

        chain.set_enabled(true);
        let back = process(&mut chain, &input);
        assert!(back.iter().zip(&input).any(|(b, i)| (b - i).abs() > 0.05), "on again, they sound");
    }
}
