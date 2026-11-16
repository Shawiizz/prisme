//! A deck's chain of effects: three slots in series, each set on its own.

mod common;

use common::{constant_track, silent_track, sine_track, Rig, BLOCK, SR};
use prisme_engine::{Command, EffectKind, Parts, Stem, FX_SLOTS};

/// An effect in a slot of deck A, at full level.
fn put(rig: &mut Rig, slot: usize, kind: EffectKind, beats: f32, amount: f32) {
    rig.send(Command::SetFx { deck: 0, slot, kind });
    rig.send(Command::SetFxBeats { deck: 0, slot, beats });
    rig.send(Command::SetFxAmount { deck: 0, slot, amount });
    rig.send(Command::SetFxMix { deck: 0, slot, mix: 1.0 });
    rig.send(Command::SetFxEnabled { deck: 0, slot, enabled: true });
}

/// A rig playing a track of a steady level on deck A.
fn steady() -> Rig {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: constant_track(10.0, 0.5) });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig
}

/// The left output for about `seconds`.
fn record(rig: &mut Rig, seconds: f64) -> Vec<f32> {
    let blocks = (seconds * SR as f64 / BLOCK as f64) as usize;
    (0..blocks)
        .flat_map(|_| {
            rig.run(BLOCK as f64 / SR as f64);
            rig.output().iter().step_by(2).copied().collect::<Vec<_>>()
        })
        .collect()
}

/// The share of `signal` that is silent.
fn silent_share(signal: &[f32]) -> f32 {
    signal.iter().filter(|v| v.abs() < 0.01).count() as f32 / signal.len() as f32
}

#[test]
fn three_slots_off_change_nothing() {
    let dry = record(&mut steady(), 1.0);
    let mut rig = steady();
    for (slot, kind) in [EffectKind::Reverb, EffectKind::Gate, EffectKind::Bitcrusher].into_iter().enumerate()
    {
        put(&mut rig, slot, kind, 0.5, 1.0);
        rig.send(Command::SetFxEnabled { deck: 0, slot, enabled: false });
    }
    assert_eq!(record(&mut rig, 1.0), dry);
}

#[test]
fn the_slots_play_in_series_in_their_order() {
    // a gate open half of each beat, and an echo half a beat later: the echo of the gated
    // sound fills its gaps, the gate after the echo leaves them silent
    let gate_then_echo = {
        let mut rig = steady();
        put(&mut rig, 0, EffectKind::Gate, 1.0, 1.0);
        put(&mut rig, 2, EffectKind::Echo, 0.5, 0.0);
        rig.run(1.0);
        silent_share(&record(&mut rig, 2.0))
    };
    let echo_then_gate = {
        let mut rig = steady();
        put(&mut rig, 0, EffectKind::Echo, 0.5, 0.0);
        put(&mut rig, 2, EffectKind::Gate, 1.0, 1.0);
        rig.run(1.0);
        silent_share(&record(&mut rig, 2.0))
    };
    assert!(gate_then_echo < 0.05, "the echo fills the gaps: {gate_then_echo}");
    assert!((0.4..0.6).contains(&echo_then_gate), "the gate cuts last: {echo_then_gate}");
}

#[test]
fn each_slot_keeps_its_own_settings() {
    let mut rig = steady();
    put(&mut rig, 0, EffectKind::Gate, 1.0, 1.0);
    for slot in 1..FX_SLOTS {
        put(&mut rig, slot, EffectKind::Gate, 8.0, 0.0);
        rig.send(Command::SetFxMix { deck: 0, slot, mix: 0.0 });
        rig.send(Command::SetFxEnabled { deck: 0, slot, enabled: false });
    }
    rig.send(Command::SetFxEnabled { deck: 0, slot: FX_SLOTS, enabled: false });
    rig.run(0.5);
    let chopped = silent_share(&record(&mut rig, 2.0));
    assert!((0.4..0.6).contains(&chopped), "the first slot still gates every beat: {chopped}");
}

/// The level of a sine with gates in the first and last slots on `parts` (the whole deck if
/// none), or without them.
fn heard(vocals_silent: bool, gates: Option<Parts>) -> f32 {
    let mut rig = Rig::new();
    let vocals = if vocals_silent { silent_track(10.0) } else { sine_track(10.0, 0.6) };
    rig.send(Command::LoadTrack { deck: 0, track: sine_track(10.0, 0.6) });
    rig.send(Command::SetStems { deck: 0, stems: [Some(vocals), None, None] });
    if let Some(parts) = gates {
        put(&mut rig, 0, EffectKind::Gate, 1.0, 1.0);
        put(&mut rig, FX_SLOTS - 1, EffectKind::Gate, 0.25, 1.0);
        rig.send(Command::SetFxParts { deck: 0, parts });
    }
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.run(1.0);
    rig.run_rms(2.0)
}

#[test]
fn the_part_takes_the_whole_chain() {
    let dry = heard(true, None);
    let on_silent_vocals = heard(true, Some(Parts::of(Stem::Vocals)));
    assert!((on_silent_vocals / dry - 1.0).abs() < 1e-4, "{on_silent_vocals} vs {dry}");
    let on_deck = heard(true, Some(Parts::NONE));
    assert!(on_deck < 0.8 * dry, "both gates chop the deck: {on_deck} vs {dry}");
    let on_vocals = heard(false, Some(Parts::of(Stem::Vocals)));
    assert!((on_vocals / on_deck - 1.0).abs() < 0.05, "{on_vocals} vs {on_deck}");
}

#[test]
fn the_second_knob_reaches_its_slot() {
    let mut rig = steady();
    put(&mut rig, 1, EffectKind::Gate, 1.0, 1.0);
    rig.send(Command::SetFxParam2 { deck: 0, slot: 1, param2: 0.25 });
    rig.run(0.5);
    let closed = silent_share(&record(&mut rig, 2.0));
    assert!((0.7..0.8).contains(&closed), "open a quarter of each beat: {closed}");
}

#[test]
fn the_chain_switched_off_and_on_again_gives_back_its_slots() {
    let dry = {
        let mut rig = steady();
        rig.run(0.5);
        record(&mut rig, 1.0)
    };
    let mut rig = steady();
    put(&mut rig, 0, EffectKind::Gate, 1.0, 1.0);
    put(&mut rig, 2, EffectKind::Bitcrusher, 1.0, 1.0);
    rig.send(Command::SetFxChainEnabled { deck: 0, enabled: false });
    rig.run(0.5);
    let bypassed = record(&mut rig, 1.0);
    assert_eq!(bypassed, dry, "nothing while the chain is off");

    rig.send(Command::SetFxChainEnabled { deck: 0, enabled: true });
    rig.run(0.5);
    let gated = silent_share(&record(&mut rig, 2.0));
    assert!((0.4..0.6).contains(&gated), "the gate is back: {gated}");
}
