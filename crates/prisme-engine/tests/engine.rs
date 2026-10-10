//! End-to-end behaviour of the engine, with a synthetic turntable on the input.

mod common;

use common::{silent_track, Rig};
use prisme_engine::{Command, VinylMode};

#[test]
fn internal_play_follows_the_pitch() {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(10.0) });
    rig.send(Command::SetPitch { deck: 0, ratio: 1.08 });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.run(1.0);
    let d = rig.deck(0);
    assert!(d.loaded && d.playing);
    assert!((d.position_s - 1.08).abs() < 0.02, "{d:?}");
    assert!((d.speed - 1.08).abs() < 1e-9);
}

#[test]
fn relative_vinyl_follows_the_record_speed() {
    let mut rig = Rig::new();
    rig.turntable(0, 20.0, 1.05);
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(30.0) });
    rig.send(Command::SetVinylMode { deck: 0, mode: VinylMode::Relative });
    rig.send(Command::SetVinylEnabled { deck: 0, enabled: true });
    rig.run(2.0);
    let d = rig.deck(0);
    assert!((d.speed - 1.05).abs() < 0.01, "{d:?}");
    // relative mode ignores the absolute position: the track started at 0
    assert!(d.position_s > 1.9 && d.position_s < 2.2, "{d:?}");
}

#[test]
fn absolute_vinyl_follows_the_record_position() {
    let mut rig = Rig::new();
    rig.turntable(0, 12.0, 1.0);
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(40.0) });
    rig.send(Command::SetLeadIn { deck: 0, seconds: 2.0 });
    rig.send(Command::SetVinylEnabled { deck: 0, enabled: true });
    rig.run(1.0);
    let d = rig.deck(0);
    // record at 13 s, minus 2 s of lead-in
    assert!((d.position_s - 11.0).abs() < 0.02, "{d:?}");
    let record = d.vinyl.record_position_s.expect("locked");
    assert!((record - 13.0).abs() < 0.02, "{d:?}");

    rig.turntables[0].as_mut().unwrap().generator.jump_ms(15_000.0);
    rig.run(0.5);
    let d = rig.deck(0);
    assert!((d.position_s - 26.5).abs() < 0.05, "{d:?}");
}

#[test]
fn replaced_tracks_are_freed_on_the_control_side() {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(1.0) });
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(1.0) });
    rig.send(Command::Eject { deck: 0 });
    rig.send(Command::LoadTrack { deck: 7, track: silent_track(1.0) });
    rig.run(0.01);
    assert_eq!(rig.controller.collect_garbage(), 3);
}
