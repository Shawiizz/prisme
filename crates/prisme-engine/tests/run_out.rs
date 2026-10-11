//! Reaching the end of the record while it plays: the deck plays on by itself.

mod common;

use common::{silent_track, Rig};
use prisme_engine::{Command, VinylMode};

/// A record playing at +3 % near the end of side A (its timecode ends at 625 s), with a
/// track lined up so that the record's 600 s is the track's start.
fn near_the_end(mode: VinylMode) -> Rig {
    let mut rig = Rig::new();
    rig.turntable(0, 622.0, 1.03);
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(60.0) });
    rig.send(Command::SetLeadIn { deck: 0, seconds: 600.0 });
    rig.send(Command::SetVinylMode { deck: 0, mode });
    rig.send(Command::SetVinylEnabled { deck: 0, enabled: true });
    rig.run(1.0);
    assert!(!rig.deck(0).vinyl.run_out);
    rig.run(2.5);
    rig
}

#[test]
fn past_the_end_the_deck_plays_on_at_the_record_speed() {
    let mut rig = near_the_end(VinylMode::Absolute);
    let d = rig.deck(0);
    assert!(d.vinyl.run_out, "{d:?}");
    let before = d.position_s;

    // the needle runs off the timecode: no signal any more
    rig.turntables[0] = None;
    rig.run(1.0);
    let d = rig.deck(0);
    assert!((d.speed - 1.03).abs() < 0.01, "{d:?}");
    assert!((d.position_s - before - 1.03).abs() < 0.03, "{d:?}");
}

#[test]
fn absolute_mode_waits_for_the_vinyl_control_to_be_set_again() {
    let mut rig = near_the_end(VinylMode::Absolute);
    // a needle dropped back at the start would make the track jump: still playing on
    rig.turntable(0, 30.0, 1.0);
    rig.run(0.5);
    assert!(rig.deck(0).vinyl.run_out);

    rig.send(Command::SetVinylEnabled { deck: 0, enabled: true });
    rig.run(0.5);
    assert!(!rig.deck(0).vinyl.run_out);
}

#[test]
fn relative_mode_takes_the_record_back_when_the_needle_is_dropped_again() {
    let mut rig = near_the_end(VinylMode::Relative);
    assert!(rig.deck(0).vinyl.run_out);
    rig.turntables[0] = None;
    rig.run(0.5);
    let before = rig.deck(0).position_s;

    rig.turntable(0, 30.0, 0.97);
    rig.run(1.0);
    let d = rig.deck(0);
    assert!(!d.vinyl.run_out, "{d:?}");
    assert!((d.speed - 0.97).abs() < 0.01, "{d:?}");
    // relative: the track carries on from where it was, it does not jump
    assert!(d.position_s > before && d.position_s < before + 1.1, "{d:?}");
}

#[test]
fn a_new_track_hands_the_deck_back_to_the_record() {
    let mut rig = near_the_end(VinylMode::Absolute);
    rig.turntable(0, 602.0, 1.0);
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(60.0) });
    rig.run(0.5);
    let d = rig.deck(0);
    assert!(!d.vinyl.run_out, "{d:?}");
    assert!((d.position_s - 2.5).abs() < 0.05, "{d:?}");
}
