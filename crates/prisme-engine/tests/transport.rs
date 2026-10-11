//! Loops, rolls, jumps, slip and censor, in internal play and with records.

mod common;

use common::{silent_track, Rig};
use prisme_engine::{BeatGrid, Command, LoopRegion, VinylMode};

/// 120 BPM: one beat every 0.5 s, from 0.
fn playing_deck(position_s: f64) -> Rig {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(60.0) });
    rig.send(Command::SetBeatGrid { deck: 0, grid: Some(BeatGrid { bpm: 120.0, first_beat_s: 0.0 }) });
    rig.send(Command::Seek { deck: 0, seconds: position_s });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.run(0.01);
    rig
}

#[track_caller]
fn assert_close(actual: f64, expected: f64, tolerance: f64) {
    assert!((actual - expected).abs() < tolerance, "{actual} instead of {expected} (± {tolerance})");
}

#[test]
fn a_beat_loop_starts_on_the_nearest_beat_and_keeps_playing_inside() {
    let mut rig = playing_deck(10.1);
    rig.send(Command::BeatLoop { deck: 0, beats: 4.0 });
    for _ in 0..20 {
        rig.run(0.25);
        let position = rig.deck(0).position_s;
        assert!((10.0..12.0).contains(&position), "{position}");
    }
    let looping = rig.deck(0).looping;
    assert!(looping.active);
    assert_eq!(looping.region, Some(LoopRegion { start_s: 10.0, end_s: 12.0 }));
}

#[test]
fn without_quantize_the_loop_starts_where_the_track_is() {
    let mut rig = playing_deck(10.1);
    rig.send(Command::SetQuantize { enabled: false });
    rig.send(Command::BeatLoop { deck: 0, beats: 1.0 });
    rig.run(0.01);
    let region = rig.deck(0).looping.region.unwrap();
    assert_close(region.start_s, 10.11, 0.01);
    assert_close(region.length_s(), 0.5, 1e-9);
}

#[test]
fn leaving_a_loop_carries_on_after_its_end() {
    let mut rig = playing_deck(10.0);
    rig.send(Command::BeatLoop { deck: 0, beats: 2.0 });
    rig.run(1.5);
    rig.send(Command::ExitLoop { deck: 0 });
    rig.run(1.0);
    assert!(rig.deck(0).position_s > 11.0);
    assert!(!rig.deck(0).looping.active);
}

#[test]
fn reloop_goes_back_to_the_loop() {
    let mut rig = playing_deck(10.0);
    rig.send(Command::BeatLoop { deck: 0, beats: 2.0 });
    rig.send(Command::ExitLoop { deck: 0 });
    rig.run(2.0);
    rig.send(Command::Reloop { deck: 0 });
    rig.run(0.1);
    assert_close(rig.deck(0).position_s, 10.1, 0.01);
    assert!(rig.deck(0).looping.active);
}

#[test]
fn halving_a_loop_keeps_its_start() {
    let mut rig = playing_deck(10.0);
    rig.send(Command::BeatLoop { deck: 0, beats: 4.0 });
    rig.send(Command::ScaleLoop { deck: 0, factor: 0.5 });
    rig.run(0.01);
    assert_eq!(rig.deck(0).looping.region, Some(LoopRegion { start_s: 10.0, end_s: 11.0 }));
}

#[test]
fn a_loop_roll_lands_where_the_track_would_have_been() {
    let mut rig = playing_deck(10.0);
    rig.send(Command::LoopRoll { deck: 0, beats: Some(0.5) });
    let elapsed = rig.run(2.0);
    assert!(rig.deck(0).position_s < 10.25);
    assert_close(rig.deck(0).slip_position_s.unwrap(), 10.01 + elapsed, 0.01);
    rig.send(Command::LoopRoll { deck: 0, beats: None });
    rig.run(0.01);
    let d = rig.deck(0);
    assert_close(d.position_s, 10.02 + elapsed, 0.01);
    assert!(!d.looping.active && d.slip_position_s.is_none());
}

#[test]
fn slip_mode_returns_from_a_loop_to_where_the_track_would_be() {
    let mut rig = playing_deck(10.0);
    rig.send(Command::SetSlip { deck: 0, enabled: true });
    rig.send(Command::BeatLoop { deck: 0, beats: 1.0 });
    let elapsed = rig.run(3.0);
    rig.send(Command::ExitLoop { deck: 0 });
    rig.run(0.01);
    assert_close(rig.deck(0).position_s, 10.02 + elapsed, 0.01);
}

#[test]
fn censor_plays_backwards_then_lands_in_time() {
    let mut rig = playing_deck(20.0);
    rig.send(Command::Censor { deck: 0, active: true });
    let elapsed = rig.run(1.0);
    assert_close(rig.deck(0).position_s, 20.01 - elapsed, 0.02);
    rig.send(Command::Censor { deck: 0, active: false });
    rig.run(0.01);
    assert_close(rig.deck(0).position_s, 20.02 + elapsed, 0.02);
}

#[test]
fn beat_jumps_move_by_whole_beats_and_take_the_loop_along() {
    let mut rig = playing_deck(10.0);
    rig.send(Command::BeatJump { deck: 0, beats: 8.0 });
    rig.run(0.01);
    assert_close(rig.deck(0).position_s, 14.02, 0.01);

    rig.send(Command::BeatLoop { deck: 0, beats: 4.0 });
    rig.send(Command::BeatJump { deck: 0, beats: -4.0 });
    rig.run(0.01);
    assert_eq!(rig.deck(0).looping.region, Some(LoopRegion { start_s: 12.0, end_s: 14.0 }));
}

#[test]
fn a_quantized_jump_keeps_the_beat_phase() {
    let mut rig = playing_deck(10.1);
    rig.send(Command::Jump { deck: 0, seconds: 30.0 });
    rig.run(0.01);
    assert_close(rig.deck(0).position_s, 30.12, 0.01);

    rig.send(Command::SetQuantize { enabled: false });
    rig.send(Command::Jump { deck: 0, seconds: 30.0 });
    rig.run(0.01);
    assert_close(rig.deck(0).position_s, 30.01, 0.01);
}

#[test]
fn a_record_in_absolute_mode_keeps_control_of_the_position() {
    let mut rig = playing_deck(10.0);
    rig.turntable(0, 12.0, 1.0);
    rig.send(Command::SetVinylMode { deck: 0, mode: VinylMode::Absolute });
    rig.send(Command::SetVinylEnabled { deck: 0, enabled: true });
    rig.run(0.5);
    rig.send(Command::BeatLoop { deck: 0, beats: 1.0 });
    rig.send(Command::Jump { deck: 0, seconds: 40.0 });
    rig.run(0.5);
    let d = rig.deck(0);
    assert!(!d.looping.active);
    assert_close(d.position_s, 13.0, 0.05);
}

#[test]
fn a_record_in_relative_mode_loops() {
    let mut rig = playing_deck(10.0);
    rig.turntable(0, 30.0, 1.0);
    rig.send(Command::SetVinylMode { deck: 0, mode: VinylMode::Relative });
    rig.send(Command::SetVinylEnabled { deck: 0, enabled: true });
    rig.run(0.2);
    rig.send(Command::BeatLoop { deck: 0, beats: 2.0 });
    rig.run(3.0);
    let d = rig.deck(0);
    assert!(d.looping.active);
    assert!(d.looping.region.unwrap().contains(d.position_s), "{d:?}");
}

#[test]
fn loop_in_then_loop_out_loops_between_their_beats() {
    let mut rig = playing_deck(10.1);
    rig.send(Command::LoopIn { deck: 0 });
    rig.run(0.01);
    assert_eq!(rig.deck(0).looping.loop_in_s, Some(10.0));
    rig.run(1.9);
    rig.send(Command::LoopOut { deck: 0 });
    rig.run(1.0);
    let d = rig.deck(0);
    assert_eq!(d.looping.region, Some(LoopRegion { start_s: 10.0, end_s: 12.0 }));
    assert!(d.looping.active && d.looping.loop_in_s.is_none());
    assert!((10.0..12.0).contains(&d.position_s), "{}", d.position_s);
}

#[test]
fn loop_out_alone_moves_the_end_of_the_stored_loop() {
    let mut rig = playing_deck(10.1);
    rig.send(Command::BeatLoop { deck: 0, beats: 4.0 });
    rig.run(1.25);
    rig.send(Command::LoopOut { deck: 0 });
    rig.run(0.01);
    assert_eq!(rig.deck(0).looping.region, Some(LoopRegion { start_s: 10.0, end_s: 11.5 }));
}

#[test]
fn loop_out_on_the_loop_in_beat_does_nothing() {
    let mut rig = playing_deck(10.1);
    rig.send(Command::LoopIn { deck: 0 });
    rig.run(0.1);
    rig.send(Command::LoopOut { deck: 0 });
    rig.run(0.01);
    let looping = rig.deck(0).looping;
    assert!(!looping.active && looping.region.is_none());
    assert_eq!(looping.loop_in_s, Some(10.0));
}
