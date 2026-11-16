//! Brake, soft start and spinback: internal play moved as a turntable's motor and a hand move
//! a record.

mod common;

use common::{silent_track, Rig};
use prisme_engine::Command;

fn deck_at(position_s: f64, playing: bool) -> Rig {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(60.0) });
    rig.send(Command::Seek { deck: 0, seconds: position_s });
    rig.send(Command::SetPlaying { deck: 0, playing });
    rig.run(0.01);
    rig
}

#[track_caller]
fn assert_close(actual: f64, expected: f64, tolerance: f64) {
    assert!((actual - expected).abs() < tolerance, "{actual} instead of {expected} (± {tolerance})");
}

#[test]
fn a_brake_slows_a_playing_deck_to_a_stop_in_a_second() {
    for key_lock in [false, true] {
        let mut rig = deck_at(10.0, true);
        rig.send(Command::SetKeyLock { deck: 0, enabled: key_lock });
        let start = rig.deck(0).position_s;
        rig.send(Command::Brake { deck: 0 });
        rig.run(0.5);
        let d = rig.deck(0);
        assert!(d.playing);
        assert_close(d.speed, 0.5, 0.01);
        rig.run(0.6);
        let d = rig.deck(0);
        assert!(!d.playing && d.speed == 0.0, "key lock {key_lock}: {d:?}");
        // an even slowdown from full speed covers half the ground of a second at speed
        assert_close(d.position_s - start, 0.5, 0.01);
    }
}

#[test]
fn a_stopped_deck_starts_softly() {
    let mut rig = deck_at(10.0, false);
    rig.send(Command::Brake { deck: 0 });
    rig.run(0.25);
    let d = rig.deck(0);
    assert!(d.playing);
    assert_close(d.speed, 0.5, 0.01);
    rig.run(0.5);
    assert_close(rig.deck(0).speed, 1.0, 1e-9);
}

#[test]
fn braking_again_brings_the_deck_back_up_to_speed() {
    let mut rig = deck_at(10.0, true);
    rig.send(Command::Brake { deck: 0 });
    rig.run(0.5);
    rig.send(Command::Brake { deck: 0 });
    rig.run(0.3);
    let d = rig.deck(0);
    assert!(d.playing && d.speed == 1.0, "{d:?}");
}

#[test]
fn pause_and_play_leave_the_brake() {
    let mut rig = deck_at(10.0, true);
    rig.send(Command::Brake { deck: 0 });
    rig.run(0.5);
    rig.send(Command::SetPlaying { deck: 0, playing: false });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.run(0.05);
    assert_close(rig.deck(0).speed, 1.0, 1e-9);
}

#[test]
fn a_record_spun_back_runs_backwards_while_held_then_plays_on_from_there() {
    let mut rig = deck_at(20.0, true);
    rig.send(Command::SpinBack { deck: 0, held: true });
    rig.run(0.5);
    let d = rig.deck(0);
    assert_close(d.speed, -3.0, 1e-9);
    assert!(d.position_s < 19.2, "{}", d.position_s);
    rig.send(Command::SpinBack { deck: 0, held: false });
    let landed = rig.deck(0).position_s;
    rig.run(0.5);
    let d = rig.deck(0);
    assert!(d.playing && d.speed == 1.0, "{d:?}");
    assert!(d.position_s < landed + 0.5, "no slip: it plays on from where it landed");
}

#[test]
fn a_stopped_deck_spun_back_plays_once_let_go() {
    let mut rig = deck_at(20.0, false);
    rig.send(Command::SpinBack { deck: 0, held: true });
    rig.run(0.02);
    assert!(rig.deck(0).speed < 0.0, "backwards at once, never forwards first");
    rig.run(0.3);
    rig.send(Command::SpinBack { deck: 0, held: false });
    rig.run(0.5);
    assert!(rig.deck(0).playing);
}

#[test]
fn a_record_drives_the_deck_and_the_motor_does_nothing() {
    let mut rig = deck_at(20.0, false);
    rig.turntable(0, 30.0, 1.0);
    rig.send(Command::SetVinylEnabled { deck: 0, enabled: true });
    rig.run(0.3);
    rig.send(Command::Brake { deck: 0 });
    rig.run(0.5);
    assert!(!rig.deck(0).playing);
    assert!(rig.deck(0).speed > 0.9, "the record turns on");
}
