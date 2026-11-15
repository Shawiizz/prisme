//! An effect on some parts of a track: the rest plays as it is.

mod common;

use common::{silent_track, sine_track, Rig};
use prisme_engine::{Command, EffectKind, Parts, Stem, StemTracks};

/// The level of a sine played with a gate chopping `parts` (the whole deck if none), or
/// without the gate.
fn heard(stems: StemTracks, gate: Option<Parts>, key_lock: bool) -> f32 {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: sine_track(10.0, 0.6) });
    rig.send(Command::SetStems { deck: 0, stems });
    rig.send(Command::SetKeyLock { deck: 0, enabled: key_lock });
    rig.send(Command::SetPitch { deck: 0, ratio: 1.04 });
    if let Some(parts) = gate {
        rig.send(Command::SetFx { deck: 0, kind: EffectKind::Gate });
        rig.send(Command::SetFxMix { deck: 0, mix: 1.0 });
        rig.send(Command::SetFxParts { deck: 0, parts });
        rig.send(Command::SetFxEnabled { deck: 0, enabled: true });
    }
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.run(1.0);
    rig.run_rms(2.0)
}

#[test]
fn an_effect_on_a_silent_part_changes_nothing() {
    for key_lock in [false, true] {
        let silent_vocals = || [Some(silent_track(10.0)), None, None];
        let dry = heard(silent_vocals(), None, key_lock);
        let on_vocals = heard(silent_vocals(), Some(Parts::of(Stem::Vocals)), key_lock);
        let on_deck = heard(silent_vocals(), Some(Parts::NONE), key_lock);
        let tolerance = if key_lock { 0.02 } else { 1e-4 };
        assert!((on_vocals / dry - 1.0).abs() < tolerance, "key lock {key_lock}: {on_vocals} vs {dry}");
        assert!(
            on_deck < 0.9 * dry,
            "key lock {key_lock}: the gate on the deck chops it: {on_deck} vs {dry}"
        );
    }
}

#[test]
fn an_effect_on_the_part_holding_everything_acts_as_on_the_deck() {
    for key_lock in [false, true] {
        let all_drums = || [None, Some(sine_track(10.0, 0.6)), None];
        let on_deck = heard(all_drums(), Some(Parts::NONE), key_lock);
        let on_drums = heard(all_drums(), Some(Parts::of(Stem::Drums)), key_lock);
        let on_the_rest = heard(all_drums(), Some(Parts::OTHER), key_lock);
        let dry = heard(all_drums(), None, key_lock);
        assert!((on_drums / on_deck - 1.0).abs() < 0.05, "key lock {key_lock}: {on_drums} vs {on_deck}");
        assert!((on_the_rest / dry - 1.0).abs() < 0.02, "key lock {key_lock}: nothing else to chop");
    }
}
