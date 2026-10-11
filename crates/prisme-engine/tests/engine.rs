//! End-to-end behaviour of the engine, with a synthetic turntable on the input.

mod common;

use common::{silent_track, Rig, SR};
use prisme_engine::{Command, VinylMode};
use prisme_timecode::{DecoderConfig, TimecodeDecoder, SERATO_CV02_A};

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
fn a_pitch_bend_lasts_while_held() {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(10.0) });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.send(Command::SetPitchBend { deck: 0, amount: 0.04 });
    rig.run(0.1);
    assert!((rig.deck(0).speed - 1.04).abs() < 1e-9);
    rig.send(Command::SetPitchBend { deck: 0, amount: 0.0 });
    rig.run(0.1);
    assert!((rig.deck(0).speed - 1.0).abs() < 1e-9);
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
fn absolute_vinyl_closes_a_small_gap_quickly() {
    let mut rig = Rig::new();
    rig.turntable(0, 12.0, 1.0);
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(40.0) });
    rig.send(Command::SetVinylEnabled { deck: 0, enabled: true });
    rig.run(1.0);
    // the track 20 ms behind the record: too little to jump, it catches up
    rig.send(Command::SetLeadIn { deck: 0, seconds: -0.02 });
    rig.run(0.1);
    let d = rig.deck(0);
    let record = d.vinyl.record_position_s.expect("locked");
    let gap = record + 0.02 - d.position_s;
    assert!(gap.abs() < 0.001, "still {:.1} ms behind after 100 ms", gap * 1000.0);
}

#[test]
fn the_scope_and_the_level_show_the_turntable_signal() {
    let mut rig = Rig::new();
    rig.turntable(0, 20.0, 1.0);
    rig.run(0.1);
    let mut points = Vec::new();
    rig.controller.read_scope(0, &mut points);
    assert!(points.len() > 1000, "{} points", points.len());
    let radius = points.iter().map(|[l, r]| (l * l + r * r).sqrt()).fold(0.0f32, f32::max);
    assert!(radius > 0.1, "radius {radius}");
    assert!(rig.deck(0).vinyl.level > 0.05);

    let mut silent = Vec::new();
    rig.controller.read_scope(1, &mut silent);
    assert!(silent.iter().all(|&p| p == [0.0, 0.0]));
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

#[test]
fn crossed_channels_are_noticed() {
    let mut rig = Rig::new();
    rig.turntable(0, 20.0, 1.0);
    let crossed = DecoderConfig { format: &SERATO_CV02_A, sample_rate: SR, swap_channels: true };
    rig.send(Command::ReplaceDecoder { deck: 0, decoder: Box::new(TimecodeDecoder::new(crossed)) });
    rig.run(4.0);
    assert!(!rig.deck(0).vinyl.crossed, "not before a while");
    rig.run(1.5);
    let d = rig.deck(0);
    assert!(d.vinyl.crossed, "{d:?}");
}

#[test]
fn going_back_and_forth_is_not_taken_for_crossed_channels() {
    let mut rig = Rig::new();
    rig.turntable(0, 60.0, -1.0);
    for _ in 0..3 {
        rig.set_turntable_speed(0, -1.0);
        rig.run(3.0);
        rig.set_turntable_speed(0, 1.0);
        rig.run(0.5);
    }
    assert!(!rig.deck(0).vinyl.crossed);
}
