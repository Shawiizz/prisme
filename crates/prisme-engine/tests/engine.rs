//! End-to-end behaviour of the engine, with a synthetic turntable on the input.

mod common;

use common::{silent_track, sine_track, Rig, BLOCK, SR};
use prisme_engine::{create, BeatGrid, Command, EngineConfig, Rpm, StemMix, VinylMode, SAMPLE_SLOTS};
use prisme_timecode::{DecoderConfig, TimecodeDecoder, SERATO_CD, SERATO_CV02_A, SERATO_CV02_B};

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
    rig.send(Command::SetLeadIn { deck: 0, seconds: Some(2.0) });
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
fn a_record_at_45_rpm_plays_the_track_at_its_own_speed() {
    let mut rig = Rig::new();
    // 13.5 s into the record at 45 rpm: 10 s of music
    rig.turntable(0, 13.5, 1.35);
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(40.0) });
    rig.send(Command::SetRpm { deck: 0, rpm: Rpm::Rpm45 });
    rig.send(Command::SetLeadIn { deck: 0, seconds: Some(2.0) });
    rig.send(Command::SetVinylEnabled { deck: 0, enabled: true });
    rig.run(1.0);
    let d = rig.deck(0);
    assert!((d.speed - 1.0).abs() < 0.01, "{d:?}");
    assert!((d.vinyl.record_speed - 1.0).abs() < 0.01, "{d:?}");
    let record = d.vinyl.record_position_s.expect("locked");
    assert!((record - 11.0).abs() < 0.02, "{d:?}");
    assert!((d.position_s - 9.0).abs() < 0.02, "{d:?}");

    rig.send(Command::SetVinylMode { deck: 0, mode: VinylMode::Relative });
    rig.set_turntable_speed(0, 1.35 * 1.05);
    rig.run(1.0);
    assert!((rig.deck(0).speed - 1.05).abs() < 0.01, "{:?}", rig.deck(0));
}

#[test]
fn absolute_vinyl_closes_a_small_gap_quickly() {
    let mut rig = Rig::new();
    rig.turntable(0, 12.0, 1.0);
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(40.0) });
    rig.send(Command::SetVinylEnabled { deck: 0, enabled: true });
    rig.run(1.0);
    // the track 20 ms behind the record: too little to jump, it catches up
    rig.send(Command::SetLeadIn { deck: 0, seconds: Some(-0.02) });
    rig.run(0.1);
    let d = rig.deck(0);
    let record = d.vinyl.record_position_s.expect("locked");
    let gap = record + 0.02 - d.position_s;
    assert!(gap.abs() < 0.001, "still {:.1} ms behind after 100 ms", gap * 1000.0);
}

#[test]
fn internal_play_stops_at_the_end_of_the_track() {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(10.0) });
    rig.send(Command::Seek { deck: 0, seconds: 9.8 });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.run(0.5);
    let d = rig.deck(0);
    assert!(!d.playing, "{d:?}");
    assert!((d.position_s - 10.0).abs() < 0.01, "{d:?}");
}

#[test]
fn each_turntable_reads_its_own_record() {
    let mut rig = Rig::with_records([&SERATO_CV02_A, &SERATO_CD]);
    for deck in 0..2 {
        rig.turntable(deck, 40.0, 1.0);
        rig.send(Command::LoadTrack { deck, track: silent_track(60.0) });
        rig.send(Command::SetVinylEnabled { deck, enabled: true });
    }
    rig.run(1.0);
    // the vinyl's tracks start 28 s into it, the CD's at its start
    let [vinyl, cd] = [0, 1].map(|deck| rig.deck(deck));
    assert!((vinyl.position_s - 13.0).abs() < 0.02, "{vinyl:?}");
    assert!((cd.position_s - 41.0).abs() < 0.02, "{cd:?}");
}

#[test]
fn another_record_is_read_once_the_deck_decodes_it() {
    let mut rig = Rig::new();
    rig.records[0] = &SERATO_CV02_B;
    rig.turntable(0, 20.0, 1.0);
    rig.run(1.0);
    let read = rig.deck(0).vinyl.record_position_s;
    assert!(read.is_none_or(|s| (s - 21.0).abs() > 1.0), "side B read as side A: {read:?}");

    let config = EngineConfig::new(SR, 2, &SERATO_CV02_A);
    rig.send(Command::ReplaceDecoder { deck: 0, decoder: config.decoder(&SERATO_CV02_B, false) });
    rig.run(1.0);
    let record = rig.deck(0).vinyl.record_position_s.expect("locked");
    assert!((record - 22.0).abs() < 0.02, "{record}");
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
fn replaced_tracks_are_freed_where_the_control_side_chooses() {
    let mut rig = Rig::new();
    let first = silent_track(1.0);
    let freed = std::sync::Arc::downgrade(&first);
    rig.send(Command::LoadTrack { deck: 0, track: first });
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(1.0) });
    rig.run(0.01);
    let garbage = rig.controller.take_garbage();
    assert_eq!(garbage.len(), 1);
    assert!(freed.upgrade().is_some(), "kept until the garbage is dropped");
    drop(garbage);
    assert!(freed.upgrade().is_none());
}

#[test]
fn the_status_tells_how_many_commands_it_follows() {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(10.0) });
    rig.send(Command::Seek { deck: 0, seconds: 4.0 });
    assert_eq!(rig.controller.sent(), 2);
    assert_eq!(rig.controller.status().commands, 0, "the engine has not run");
    rig.run(0.01);
    let status = rig.controller.status();
    assert_eq!(status.commands, 2);
    assert!((status.decks[0].position_s - 4.0).abs() < 0.01, "{:?}", status.decks[0]);
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

#[test]
fn a_track_plays_at_its_level() {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: sine_track(10.0, 0.6) });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.run(0.5);
    let full = rig.run_rms(0.5);
    rig.send(Command::SetLevel { deck: 0, gain: 0.5 });
    rig.run(0.5);
    let half = rig.run_rms(0.5);
    assert!((half / full - 0.5).abs() < 1e-3, "{half} / {full}");
}

#[test]
fn a_double_plays_from_where_its_source_is() {
    let mut rig = Rig::new();
    let track = sine_track(10.0, 0.6);
    rig.send(Command::LoadTrack { deck: 0, track: track.clone() });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.run(1.5);
    rig.send(Command::LoadTrack { deck: 1, track });
    rig.send(Command::TakePlace { deck: 1, from: 0 });
    rig.run(0.5);
    let (a, b) = (rig.deck(0), rig.deck(1));
    assert!(b.playing, "{b:?}");
    assert!((a.position_s - b.position_s).abs() < 0.01, "{a:?} {b:?}");
}

#[test]
fn a_double_of_a_deck_in_a_loop_loops_with_it() {
    let mut rig = Rig::new();
    let track = sine_track(10.0, 0.6);
    rig.send(Command::LoadTrack { deck: 0, track: track.clone() });
    rig.send(Command::SetBeatGrid { deck: 0, grid: Some(BeatGrid { bpm: 120.0, first_beat_s: 0.0 }) });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.run(1.1);
    rig.send(Command::BeatLoop { deck: 0, beats: 1.0 });
    rig.run(0.2);
    rig.send(Command::LoadTrack { deck: 1, track });
    rig.send(Command::TakePlace { deck: 1, from: 0 });
    rig.run(1.3);
    let (a, b) = (rig.deck(0), rig.deck(1));
    assert!(b.looping.active && a.position_s < 2.0, "{a:?} {b:?}");
    assert!((a.position_s - b.position_s).abs() < 0.01, "{a:?} {b:?}");
}

#[test]
fn a_stem_muted_on_the_bar_waits_for_it() {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: sine_track(10.0, 0.6) });
    rig.send(Command::SetStems { deck: 0, stems: [Some(sine_track(10.0, 0.6)), None, None] });
    // a bar every 2 s
    rig.send(Command::SetBeatGrid { deck: 0, grid: Some(BeatGrid { bpm: 120.0, first_beat_s: 0.0 }) });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.run(0.5);
    let full = rig.run_rms(0.25);
    let muted = StemMix { vocals: 0.0, ..StemMix::default() };
    rig.send(Command::SetStemMix { deck: 0, mix: muted, on_bar: true });
    rig.run(0.25);
    assert_eq!(rig.deck(0).stems_waiting, 0b0001);
    let waiting = rig.run_rms(0.9);
    assert!((waiting / full - 1.0).abs() < 1e-3, "{waiting} / {full}");
    rig.run(0.2);
    let d = rig.deck(0);
    assert!(d.stems_waiting == 0 && d.position_s > 2.0, "{d:?}");
    assert!(rig.run_rms(0.25) < full * 1e-3);
}

#[test]
fn a_stem_muted_on_the_bar_inside_a_shorter_loop_waits_for_the_loop_to_go_round() {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: sine_track(10.0, 0.6) });
    rig.send(Command::SetStems { deck: 0, stems: [Some(sine_track(10.0, 0.6)), None, None] });
    rig.send(Command::SetBeatGrid { deck: 0, grid: Some(BeatGrid { bpm: 120.0, first_beat_s: 0.0 }) });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.run(0.6);
    // one beat, 0.5 s
    rig.send(Command::BeatLoop { deck: 0, beats: 1.0 });
    rig.run(0.1);
    let muted = StemMix { vocals: 0.0, ..StemMix::default() };
    rig.send(Command::SetStemMix { deck: 0, mix: muted, on_bar: true });
    rig.run(0.05);
    assert_eq!(rig.deck(0).stems_waiting, 0b0001);
    rig.run(0.6);
    let d = rig.deck(0);
    assert!(d.stems_waiting == 0 && d.position_s < 2.0, "{d:?}");
}

#[test]
fn a_double_of_a_deck_in_a_roll_plays_on_where_the_roll_comes_back() {
    let mut rig = Rig::new();
    let track = sine_track(10.0, 0.6);
    rig.send(Command::LoadTrack { deck: 0, track: track.clone() });
    rig.send(Command::SetBeatGrid { deck: 0, grid: Some(BeatGrid { bpm: 120.0, first_beat_s: 0.0 }) });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.run(1.0);
    rig.send(Command::LoopRoll { deck: 0, beats: Some(0.125) });
    rig.run(0.5);
    rig.send(Command::LoadTrack { deck: 1, track });
    rig.send(Command::TakePlace { deck: 1, from: 0 });
    rig.run(0.25);
    let (a, b) = (rig.deck(0), rig.deck(1));
    let shadow = a.slip_position_s.expect("rolling");
    assert!(!b.looping.active, "{b:?}");
    assert!((b.position_s - shadow).abs() < 0.01, "{a:?} {b:?}");
}

#[test]
fn a_whole_session_restored_at_once_fits_in_the_queue() {
    // what the app sends as the audio starts again: for each of four decks its track (6
    // commands), its settings (41), its record and its pads, then the mixer, the live inputs
    // and where each deck was (29)
    let commands = 4 * (6 + 41 + 1 + SAMPLE_SLOTS) + 29;
    let (_engine, mut controller) = create(EngineConfig::new(SR, 4, &SERATO_CV02_A));
    for _ in 0..commands {
        assert!(controller.send(Command::SetQuantize { enabled: true }).is_ok());
    }
}

#[test]
fn positions_and_tempos_out_of_range_are_ignored() {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: sine_track(10.0, 0.6) });
    rig.send(Command::SetBeatGrid { deck: 0, grid: Some(BeatGrid { bpm: 0.0, first_beat_s: 0.0 }) });
    rig.send(Command::Seek { deck: 0, seconds: f64::INFINITY });
    rig.send(Command::SetPitch { deck: 0, ratio: f64::NAN });
    rig.send(Command::BeatJump { deck: 0, beats: 4.0 });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.run(0.5);
    let d = rig.deck(0);
    assert!(d.bpm.is_none() && (d.position_s - 0.5).abs() < 0.02, "{d:?}");
}

#[test]
fn a_deck_mutes_any_stem() {
    for key_lock in [false, true] {
        let mut rig = Rig::new();
        rig.send(Command::LoadTrack { deck: 0, track: sine_track(10.0, 0.6) });
        let stems = [Some(sine_track(10.0, 0.2)), Some(sine_track(10.0, 0.15)), Some(sine_track(10.0, 0.1))];
        rig.send(Command::SetStems { deck: 0, stems });
        rig.send(Command::SetKeyLock { deck: 0, enabled: key_lock });
        rig.send(Command::SetPlaying { deck: 0, playing: true });
        rig.run(0.5);
        let both = rig.run_rms(0.5);
        assert!(both > 0.05, "{both}");
        let level = |rig: &mut Rig, mix: StemMix| {
            rig.send(Command::SetStemMix { deck: 0, mix, on_bar: false });
            rig.run(0.5);
            rig.run_rms(0.5) / both
        };
        let tolerance = if key_lock { 0.01 } else { 1e-3 };
        let gain = |on: bool| if on { 1.0 } else { 0.0 };
        let only = |vocals, drums, bass, other| StemMix {
            vocals: gain(vocals),
            drums: gain(drums),
            bass: gain(bass),
            other: gain(other),
        };
        for (mix, expected) in [
            (only(false, true, true, true), 0.4),
            (only(true, false, false, false), 0.2),
            (only(false, true, false, false), 0.15),
            (only(false, false, true, false), 0.1),
            (only(false, false, false, true), 0.15),
        ] {
            let got = level(&mut rig, mix);
            assert!((got - expected / 0.6).abs() < tolerance, "key lock {key_lock}, {mix:?}: {got}");
        }
        assert!(level(&mut rig, only(false, false, false, false)) < 1e-3);

        // another track plays whole, whatever the previous one muted
        rig.send(Command::LoadTrack { deck: 0, track: sine_track(10.0, 0.6) });
        rig.send(Command::SetPlaying { deck: 0, playing: true });
        rig.run(0.5);
        assert!((rig.run_rms(0.5) / both - 1.0).abs() < tolerance, "key lock {key_lock}");
    }
}

#[test]
fn a_touched_jog_carries_the_track_like_a_record() {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(60.0) });
    rig.send(Command::Seek { deck: 0, seconds: 10.0 });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.run(0.2);
    let before = rig.deck(0).position_s;
    rig.send(Command::JogTouch { deck: 0, touched: true, at_s: 100.0 });
    // half a turn forwards over 0.1 s, then held still
    for i in 1..=10 {
        rig.send(Command::Jog { deck: 0, turns: 0.05, at_s: 100.0 + 0.01 * f64::from(i) });
        rig.run(0.01);
    }
    rig.run(0.3);
    let held = rig.deck(0).position_s;
    assert!((held - (before + 0.9)).abs() < 0.03, "{before} -> {held}");
    rig.run(0.3);
    assert!((rig.deck(0).position_s - held).abs() < 1e-6, "held still, the track stays");

    rig.send(Command::JogTouch { deck: 0, touched: false, at_s: 101.0 });
    rig.run(0.5);
    assert!((rig.deck(0).position_s - (held + 0.5)).abs() < 0.03, "let go, it plays on");
}

#[test]
fn a_track_scratched_in_a_loop_goes_round_it() {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(60.0) });
    rig.send(Command::SetBeatGrid { deck: 0, grid: Some(BeatGrid { bpm: 120.0, first_beat_s: 0.0 }) });
    rig.send(Command::Seek { deck: 0, seconds: 10.0 });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.send(Command::BeatLoop { deck: 0, beats: 1.0 });
    rig.run(0.2);
    let region = rig.deck(0).looping.region.unwrap();
    let before = rig.deck(0).position_s;
    let in_loop = |rig: &mut Rig| {
        let position = rig.deck(0).position_s;
        assert!(region.contains(position), "{position} out of {region:?}");
        position
    };
    rig.send(Command::JogTouch { deck: 0, touched: true, at_s: 100.0 });
    // a quarter turn forwards over 0.1 s, past the loop's end, then held still: played all the
    // way, not jumped
    let block_s = BLOCK as f64 / SR as f64;
    let mut played = 0.0;
    for i in 0..80 {
        if i % 2 == 0 && i < 20 {
            rig.send(Command::Jog { deck: 0, turns: 0.025, at_s: 100.0 + block_s * f64::from(i + 2) });
        }
        rig.run(block_s);
        played += rig.deck(0).speed * block_s;
        in_loop(&mut rig);
    }
    assert!((played - 0.45).abs() < 0.03, "{played}");
    let held = in_loop(&mut rig);
    assert!((held - region.wrap(before + 0.45)).abs() < 0.03, "{before} -> {held}");

    rig.send(Command::JogTouch { deck: 0, touched: false, at_s: 101.0 });
    for _ in 0..10 {
        rig.run(0.1);
        in_loop(&mut rig);
    }
}

#[test]
fn a_track_flung_by_hand_past_the_end_of_its_loop_lands_round_it() {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(60.0) });
    rig.send(Command::SetBeatGrid { deck: 0, grid: Some(BeatGrid { bpm: 120.0, first_beat_s: 0.0 }) });
    rig.send(Command::Seek { deck: 0, seconds: 10.0 });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.send(Command::BeatLoop { deck: 0, beats: 1.0 });
    rig.run(0.2);
    let region = rig.deck(0).looping.region.unwrap();
    let taken = rig.deck(0).position_s;
    rig.send(Command::Hand { deck: 0, position_s: Some(taken), at_s: 50.0 });
    rig.run(0.05);
    // three seconds of the track in 60 ms
    rig.send(Command::Hand { deck: 0, position_s: Some(taken + 3.1), at_s: 50.06 });
    rig.run(0.1);
    let landed = rig.deck(0).position_s;
    assert!((landed - region.wrap(taken + 3.1)).abs() < 0.01, "{taken} -> {landed} in {region:?}");

    rig.send(Command::Hand { deck: 0, position_s: None, at_s: 50.2 });
    for _ in 0..10 {
        rig.run(0.1);
        let position = rig.deck(0).position_s;
        assert!(region.contains(position), "{position} out of {region:?}");
    }
}

#[test]
fn a_press_on_the_overview_takes_the_track_out_of_its_loop() {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(60.0) });
    rig.send(Command::SetBeatGrid { deck: 0, grid: Some(BeatGrid { bpm: 120.0, first_beat_s: 0.0 }) });
    rig.send(Command::Seek { deck: 0, seconds: 10.0 });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.send(Command::BeatLoop { deck: 0, beats: 1.0 });
    rig.run(0.2);
    rig.send(Command::Hand { deck: 0, position_s: Some(40.0), at_s: 50.0 });
    rig.run(0.1);
    assert!((rig.deck(0).position_s - 40.0).abs() < 0.01, "{:?}", rig.deck(0));
}

#[test]
fn the_jog_and_the_screen_hold_the_track_each_on_its_own() {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(60.0) });
    rig.send(Command::Seek { deck: 0, seconds: 10.0 });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.run(0.2);
    let held = rig.deck(0).position_s;
    rig.send(Command::JogTouch { deck: 0, touched: true, at_s: 100.0 });
    rig.send(Command::Hand { deck: 0, position_s: Some(held), at_s: 50.0 });
    rig.run(0.3);
    rig.send(Command::Hand { deck: 0, position_s: None, at_s: 50.3 });
    rig.run(0.3);
    assert!((rig.deck(0).position_s - held).abs() < 0.01, "the jog still holds it");

    rig.send(Command::JogTouch { deck: 0, touched: false, at_s: 100.6 });
    rig.run(0.5);
    assert!((rig.deck(0).position_s - (held + 0.5)).abs() < 0.03, "let go, it plays on");
}

#[test]
fn the_jog_edge_nudges_a_playing_track() {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(60.0) });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.run(0.2);
    let before = rig.deck(0).position_s;
    rig.send(Command::Jog { deck: 0, turns: 0.2, at_s: 0.0 });
    rig.run(0.05);
    assert!(rig.deck(0).speed > 1.05, "{:?}", rig.deck(0).speed);
    rig.run(1.0);
    let moved = rig.deck(0).position_s - before;
    // +20 % fading over 0.1 s: about 0.02 s ahead of plain play
    assert!(moved > 1.05 + 0.01 && moved < 1.05 + 0.03, "{moved}");
    assert!((rig.deck(0).speed - 1.0).abs() < 1e-3);
}

#[test]
fn the_jog_moves_a_stopped_track_and_leaves_a_record_alone() {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(60.0) });
    rig.send(Command::Seek { deck: 0, seconds: 10.0 });
    rig.send(Command::Jog { deck: 0, turns: -1.0, at_s: 0.0 });
    rig.run(0.05);
    assert!((rig.deck(0).position_s - 8.2).abs() < 1e-6);

    rig.turntable(1, 20.0, 1.0);
    rig.send(Command::LoadTrack { deck: 1, track: silent_track(60.0) });
    rig.send(Command::SetVinylEnabled { deck: 1, enabled: true });
    rig.run(0.5);
    let record = rig.deck(1).position_s;
    rig.send(Command::JogTouch { deck: 1, touched: true, at_s: 0.0 });
    rig.send(Command::Jog { deck: 1, turns: 2.0, at_s: 0.01 });
    rig.run(0.5);
    assert!((rig.deck(1).position_s - (record + 0.5)).abs() < 0.05, "the record drives");
}
