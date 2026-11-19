//! Loops, rolls, jumps, slip and censor, in internal play and with records.

mod common;

use common::{silent_track, Rig};
use prisme_engine::{BeatGrid, Command, LoopRegion, VinylMode};
use prisme_media::TrackAudio;
use std::sync::Arc;

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
fn a_loop_past_the_end_of_the_track_goes_on_round() {
    let mut rig = playing_deck(58.9);
    rig.send(Command::BeatLoop { deck: 0, beats: 4.0 });
    for _ in 0..12 {
        rig.run(0.25);
        let d = rig.deck(0);
        assert!(d.playing && (58.9..61.0).contains(&d.position_s), "{d:?}");
    }
    assert_eq!(rig.deck(0).looping.region, Some(LoopRegion { start_s: 59.0, end_s: 61.0 }));
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
fn halving_a_loop_behind_the_playhead_takes_it_back_in_phase() {
    let mut rig = playing_deck(10.0);
    rig.send(Command::BeatLoop { deck: 0, beats: 4.0 });
    rig.run(1.5);
    let before = rig.deck(0).position_s;
    rig.send(Command::ScaleLoop { deck: 0, factor: 0.5 });
    let elapsed = rig.run(0.01);
    assert_close(rig.deck(0).position_s, before - 1.0 + elapsed, 1e-6);
    for _ in 0..8 {
        rig.run(0.25);
        let position = rig.deck(0).position_s;
        assert!((10.0..11.0).contains(&position), "{position}");
    }
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
fn a_roll_in_a_loop_lets_go_back_round_that_loop() {
    let outer = LoopRegion { start_s: 10.0, end_s: 14.0 };
    for slip in [false, true] {
        let mut rig = playing_deck(10.0);
        rig.send(Command::SetSlip { deck: 0, enabled: slip });
        rig.send(Command::BeatLoop { deck: 0, beats: 8.0 });
        let looped = rig.run(3.0);
        rig.send(Command::LoopRoll { deck: 0, beats: Some(0.25) });
        let rolled = rig.run(2.0);
        rig.send(Command::LoopRoll { deck: 0, beats: None });
        let after = rig.run(0.01);
        let d = rig.deck(0);
        let track_s = 10.01 + looped + rolled + after;
        assert!(d.looping.active && d.looping.region == Some(outer), "slip {slip}: {d:?}");
        assert_close(d.position_s, outer.wrap(track_s), 0.01);
        assert_eq!(d.slip_position_s.is_some(), slip, "the loop's own slip goes on");

        rig.send(Command::ExitLoop { deck: 0 });
        rig.run(0.01);
        let landing = if slip { track_s + 0.01 } else { outer.wrap(track_s) + 0.01 };
        assert_close(rig.deck(0).position_s, landing, 0.01);
    }
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
fn censor_or_reverse_in_a_loop_lands_round_it_in_time() {
    for (slip, censor) in [(false, true), (true, true), (true, false)] {
        let mut rig = playing_deck(10.0);
        rig.send(Command::SetSlip { deck: 0, enabled: slip });
        rig.send(Command::BeatLoop { deck: 0, beats: 2.0 });
        rig.run(0.3);
        let region = rig.deck(0).looping.region.unwrap();
        let before = rig.deck(0).position_s;
        let backwards = |on| match censor {
            true => Command::Censor { deck: 0, active: on },
            false => Command::SetReverse { deck: 0, enabled: on },
        };
        rig.send(backwards(true));
        let elapsed = rig.run(1.2);
        rig.send(backwards(false));
        let elapsed = elapsed + rig.run(0.01);
        let d = rig.deck(0);
        assert!(d.looping.active, "slip {slip}, censor {censor}");
        assert_close(d.position_s, region.wrap(before + elapsed), 0.02);
        // with slip, the loop's own goes on
        assert_eq!(d.slip_position_s.is_some(), slip);
    }
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
    rig.send(Command::SetLeadIn { deck: 0, seconds: Some(0.0) });
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
fn a_record_in_absolute_mode_is_taken_round_by_no_loop_nor_repeat() {
    // heard up to 10.5 s, silent after, to its end at 11 s
    let track = |s: f64| {
        (0..(11.0 * 44_100.0) as usize).map(move |i| if i < (s * 44_100.0) as usize { 0.5 } else { 0.0 })
    };
    let track = Arc::new(TrackAudio::from_interleaved(44_100, track(10.5).flat_map(|v| [v, v]).collect()));
    let loop_region = LoopRegion { start_s: 10.0, end_s: 10.5 };
    for (looping, repeat) in [(true, false), (false, true)] {
        let mut rig = Rig::new();
        rig.turntable(0, 10.0, 1.0);
        rig.send(Command::LoadTrack { deck: 0, track: track.clone() });
        rig.send(Command::SetLoop { deck: 0, region: Some(loop_region), active: looping });
        rig.send(Command::SetRepeat { deck: 0, enabled: repeat });
        rig.send(Command::SetVinylMode { deck: 0, mode: VinylMode::Absolute });
        rig.send(Command::SetLeadIn { deck: 0, seconds: Some(0.0) });
        rig.send(Command::SetVinylEnabled { deck: 0, enabled: true });
        rig.run(0.2);
        let mut silent = false;
        for _ in 0..200 {
            rig.run(0.005);
            for &sample in rig.output().iter().step_by(2) {
                silent |= sample.abs() < 0.05;
                assert!(!silent || sample.abs() < 0.25, "heard again past 10.5 s, looping {looping}");
            }
        }
        assert!(silent && rig.deck(0).position_s > 11.0, "{:?}", rig.deck(0));
    }
}

#[test]
fn a_record_in_absolute_mode_starts_the_track_past_its_own_lead_in() {
    let mut rig = playing_deck(10.0);
    // a Serato record: the track starts 28 s into it
    rig.turntable(0, 30.0, 1.0);
    rig.send(Command::SetVinylMode { deck: 0, mode: VinylMode::Absolute });
    rig.send(Command::SetVinylEnabled { deck: 0, enabled: true });
    rig.run(0.5);
    assert_close(rig.deck(0).position_s, 2.5, 0.05);
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

#[test]
fn with_repeat_the_track_plays_on_from_its_start() {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(10.0) });
    rig.send(Command::SetRepeat { deck: 0, enabled: true });
    rig.send(Command::Seek { deck: 0, seconds: 9.5 });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    let elapsed = rig.run(1.0);
    let d = rig.deck(0);
    assert!(d.playing);
    assert_close(d.position_s, 9.5 + elapsed - 10.0, 0.001);

    rig.send(Command::SetRepeat { deck: 0, enabled: false });
    rig.send(Command::Seek { deck: 0, seconds: 9.5 });
    rig.run(1.0);
    assert!(!rig.deck(0).playing, "without it, the end stops the deck");
}

#[test]
fn a_slice_plays_round_while_held_in_phase_then_the_track_comes_back() {
    let mut rig = playing_deck(10.1);
    let slice = LoopRegion { start_s: 12.0, end_s: 12.5 };
    rig.send(Command::Slice { deck: 0, region: Some(slice) });
    rig.run(0.01);
    assert_close(rig.deck(0).position_s, 12.12, 0.01);
    let elapsed = rig.run(2.0);
    let d = rig.deck(0);
    assert!(slice.contains(d.position_s), "{}", d.position_s);
    assert_close(d.slip_position_s.unwrap(), 10.12 + elapsed, 0.01);

    let next = LoopRegion { start_s: 13.0, end_s: 13.5 };
    rig.send(Command::Slice { deck: 0, region: Some(next) });
    rig.run(0.01);
    assert!(next.contains(rig.deck(0).position_s), "another slice taken while held");
    rig.send(Command::Slice { deck: 0, region: None });
    rig.run(0.01);
    let d = rig.deck(0);
    assert_close(d.position_s, 10.14 + elapsed, 0.01);
    assert!(!d.looping.active && d.slip_position_s.is_none());
}

#[test]
fn without_quantize_a_slice_plays_from_its_start() {
    let mut rig = playing_deck(10.1);
    rig.send(Command::SetQuantize { enabled: false });
    rig.send(Command::Slice { deck: 0, region: Some(LoopRegion { start_s: 12.0, end_s: 12.5 }) });
    rig.run(0.01);
    assert_close(rig.deck(0).position_s, 12.01, 0.01);
}

#[test]
fn reverse_plays_backwards_at_the_deck_rate_and_stops_at_the_start() {
    let mut rig = playing_deck(20.0);
    rig.send(Command::SetPitch { deck: 0, ratio: 1.5 });
    rig.send(Command::SetReverse { deck: 0, enabled: true });
    let elapsed = rig.run(1.0);
    let d = rig.deck(0);
    assert!(d.reverse && d.speed < 0.0);
    assert_close(d.position_s, 20.01 - 1.5 * elapsed, 0.02);

    rig.send(Command::Seek { deck: 0, seconds: 0.5 });
    rig.run(1.0);
    let d = rig.deck(0);
    assert!(!d.playing, "the start stops it");
    assert_close(d.position_s, 0.0, 1e-9);

    rig.send(Command::SetReverse { deck: 0, enabled: false });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.run(0.5);
    assert!(rig.deck(0).position_s > 0.7);
}

#[test]
fn reverse_held_goes_back_to_forward_from_where_it_is_or_with_slip_where_it_would_be() {
    let mut rig = playing_deck(20.0);
    rig.send(Command::HoldReverse { deck: 0, held: true });
    let elapsed = rig.run(1.0);
    assert!(!rig.deck(0).reverse, "held, the switch stays off");
    rig.send(Command::HoldReverse { deck: 0, held: false });
    rig.run(0.01);
    let d = rig.deck(0);
    assert!(d.speed > 0.0);
    assert_close(d.position_s, 20.02 - elapsed, 0.02);

    let mut rig = playing_deck(20.0);
    rig.send(Command::SetSlip { deck: 0, enabled: true });
    rig.send(Command::HoldReverse { deck: 0, held: true });
    let elapsed = rig.run(1.0);
    rig.send(Command::HoldReverse { deck: 0, held: false });
    rig.run(0.01);
    assert_close(rig.deck(0).position_s, 20.02 + elapsed, 0.02);
}

#[test]
fn reverse_goes_round_a_loop_and_with_repeat_from_the_start_to_the_end() {
    let mut rig = playing_deck(10.1);
    rig.send(Command::BeatLoop { deck: 0, beats: 2.0 });
    rig.send(Command::SetReverse { deck: 0, enabled: true });
    for _ in 0..12 {
        rig.run(0.25);
        let position = rig.deck(0).position_s;
        assert!((10.0..=11.0).contains(&position), "{position}");
    }

    let mut rig = playing_deck(0.2);
    rig.send(Command::SetRepeat { deck: 0, enabled: true });
    rig.send(Command::SetReverse { deck: 0, enabled: true });
    let elapsed = rig.run(0.5);
    let d = rig.deck(0);
    assert!(d.playing);
    assert_close(d.position_s, 60.0 + 0.21 - elapsed, 0.02);
}

#[test]
fn a_reversed_deck_follows_the_leader_tempo_without_chasing_its_beats() {
    let mut rig = playing_deck(20.0);
    rig.send(Command::LoadTrack { deck: 1, track: silent_track(60.0) });
    rig.send(Command::SetBeatGrid { deck: 1, grid: Some(BeatGrid { bpm: 125.0, first_beat_s: 0.0 }) });
    rig.send(Command::SetPlaying { deck: 1, playing: true });
    rig.run(0.1);
    rig.send(Command::SetSyncLocked { deck: 0, locked: true });
    rig.run(0.5);
    rig.send(Command::SetReverse { deck: 0, enabled: true });
    let before = rig.deck(0).position_s;
    let elapsed = rig.run(2.0);
    let d = rig.deck(0);
    assert!(rig.deck(1).sync.leader && !d.sync.leader);
    assert_close(d.position_s, before - elapsed * 125.0 / 120.0, 0.02);
}
