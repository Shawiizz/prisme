//! Tempo and beat sync between two decks, in internal play and with records.

mod common;

use common::{silent_track, Rig};
use prisme_engine::{BeatGrid, Command, LoopRegion, VinylMode};

const LEADER_BPM: f64 = 128.0;

fn load(rig: &mut Rig, deck: usize, bpm: f64, first_beat_s: f64) {
    load_with_bars(rig, deck, bpm, first_beat_s, true);
}

fn load_with_bars(rig: &mut Rig, deck: usize, bpm: f64, first_beat_s: f64, bars_sure: bool) {
    rig.send(Command::LoadTrack { deck, track: silent_track(60.0) });
    rig.send(Command::SetBeatGrid { deck, grid: Some(BeatGrid::steady(bpm, first_beat_s, bars_sure)) });
}

fn play_internal(rig: &mut Rig, deck: usize, position_s: f64) {
    rig.send(Command::Seek { deck, seconds: position_s });
    rig.send(Command::SetPlaying { deck, playing: true });
}

fn relative_record(rig: &mut Rig, deck: usize, speed: f64) {
    rig.turntable(deck, 20.0, speed);
    rig.send(Command::SetVinylMode { deck, mode: VinylMode::Relative });
    rig.send(Command::SetVinylEnabled { deck, enabled: true });
}

/// Deck 1's beats minus deck 0's, in beats, wrapped to `[-0.5, 0.5]`.
fn phase_gap(rig: &mut Rig) -> f64 {
    let gap = rig.deck(1).beat_phase.unwrap() - rig.deck(0).beat_phase.unwrap();
    gap - gap.round()
}

/// Deck 1's beats (a 125 BPM grid from 0.31 s) minus deck 0's (the leader's from 0), each
/// counted from its grid's first beat, a bar's first, wrapped to `[-2, 2]`.
fn bar_gap(rig: &mut Rig) -> f64 {
    let follower = (rig.deck(1).position_s - 0.31) * 125.0 / 60.0;
    let gap = follower - rig.deck(0).position_s * LEADER_BPM / 60.0;
    gap - 4.0 * (gap / 4.0).round()
}

/// Mean speed of `deck` over the next `seconds`, from how far it moves.
fn mean_speed(rig: &mut Rig, deck: usize, seconds: f64) -> f64 {
    let start = rig.deck(deck).position_s;
    let elapsed = rig.run(seconds);
    (rig.deck(deck).position_s - start) / elapsed
}

#[track_caller]
fn assert_close(actual: f64, expected: f64, tolerance: f64) {
    assert!((actual - expected).abs() < tolerance, "{actual} instead of {expected} (± {tolerance})");
}

fn leader_with_follower(follower_bpm: f64) -> Rig {
    let mut rig = Rig::new();
    load(&mut rig, 0, LEADER_BPM, 0.0);
    load(&mut rig, 1, follower_bpm, 0.31);
    play_internal(&mut rig, 0, 0.0);
    rig.run(1.0);
    rig
}

#[test]
fn an_internal_follower_locks_to_the_tempo_and_the_beats() {
    let mut rig = leader_with_follower(125.0);
    play_internal(&mut rig, 1, 10.07);
    rig.send(Command::SetSyncLocked { deck: 1, locked: true });
    rig.run(1.0);

    let follower = rig.deck(1);
    assert!(follower.sync.locked && rig.deck(0).sync.leader);
    assert_close(mean_speed(&mut rig, 1, 2.0), LEADER_BPM / 125.0, 1e-3);
    assert!(phase_gap(&mut rig).abs() < 0.01, "{}", phase_gap(&mut rig));

    rig.send(Command::SetPitch { deck: 0, ratio: 1.05 });
    rig.run(1.0);
    assert_close(mean_speed(&mut rig, 1, 2.0), 1.05 * LEADER_BPM / 125.0, 2e-3);
    assert!(phase_gap(&mut rig).abs() < 0.02, "{}", phase_gap(&mut rig));
}

#[test]
fn sync_lays_the_bars_on_the_leader_s_then_lets_a_beat_jump_be() {
    let mut rig = leader_with_follower(125.0);
    // two beats into a bar of its own as the leader starts one
    play_internal(&mut rig, 1, 10.07);
    rig.send(Command::SetSyncLocked { deck: 1, locked: true });
    rig.run(1.0);
    assert!(bar_gap(&mut rig).abs() < 0.02, "{}", bar_gap(&mut rig));

    let at = rig.deck(1).position_s;
    rig.send(Command::Seek { deck: 1, seconds: at + 60.0 / 125.0 });
    rig.run(1.0);
    assert_close(bar_gap(&mut rig), 1.0, 0.02);
}

#[test]
fn sync_lays_only_the_beats_of_a_track_whose_bars_are_unsure() {
    let mut rig = Rig::new();
    load(&mut rig, 0, LEADER_BPM, 0.0);
    load_with_bars(&mut rig, 1, 125.0, 0.31, false);
    play_internal(&mut rig, 0, 0.0);
    rig.run(1.0);
    play_internal(&mut rig, 1, 10.07);
    rig.send(Command::SetSyncLocked { deck: 1, locked: true });
    rig.run(1.0);
    assert!(phase_gap(&mut rig).abs() < 0.01, "{}", phase_gap(&mut rig));
    assert_close(bar_gap(&mut rig).abs(), 2.0, 0.02);
}

#[test]
fn an_internal_deck_matched_once_takes_the_beats_and_stays_free() {
    let mut rig = leader_with_follower(125.0);
    play_internal(&mut rig, 1, 10.07);
    rig.send(Command::SetPitch { deck: 1, ratio: LEADER_BPM / 125.0 });
    rig.send(Command::SyncOnce { deck: 1 });
    rig.run(0.5);
    let follower = rig.deck(1);
    assert!(!follower.sync.locked, "{follower:?}");
    assert!(phase_gap(&mut rig).abs() < 0.01, "{}", phase_gap(&mut rig));
    // free: a pitch change is played, not undone by the leader
    rig.send(Command::SetPitch { deck: 1, ratio: 1.1 });
    rig.run(0.2);
    assert_close(mean_speed(&mut rig, 1, 1.0), 1.1, 1e-3);
}

#[test]
fn a_follower_plays_on_when_the_leader_stops() {
    let mut rig = leader_with_follower(125.0);
    play_internal(&mut rig, 1, 10.07);
    rig.send(Command::SetSyncLocked { deck: 1, locked: true });
    rig.run(1.0);
    rig.send(Command::SetPlaying { deck: 0, playing: false });
    let speed = LEADER_BPM / 125.0;
    let mut position = rig.deck(1).position_s;
    for _ in 0..200 {
        let elapsed = rig.run(0.01);
        let now = rig.deck(1).position_s;
        assert_close(now - position, speed * elapsed, 1e-4);
        position = now;
    }
}

#[test]
fn a_follower_plays_on_through_the_leader_s_short_loops() {
    let mut rig = leader_with_follower(125.0);
    play_internal(&mut rig, 1, 10.07);
    rig.send(Command::SetSyncLocked { deck: 1, locked: true });
    rig.run(1.0);
    // half a beat, looped: the leader goes back half a beat at every turn
    rig.send(Command::BeatLoop { deck: 0, beats: 0.5 });
    let speed = LEADER_BPM / 125.0;
    let mut position = rig.deck(1).position_s;
    for _ in 0..200 {
        let elapsed = rig.run(0.01);
        let now = rig.deck(1).position_s;
        assert_close(now - position, speed * elapsed, 1e-4);
        position = now;
    }
}

/// The follower deck 1 locked to deck 0, the leader.
fn locked_follower() -> Rig {
    let mut rig = leader_with_follower(125.0);
    play_internal(&mut rig, 1, 10.07);
    rig.send(Command::SetSyncLocked { deck: 1, locked: true });
    rig.run(1.0);
    rig
}

#[test]
fn a_follower_goes_round_its_own_loops_shorter_than_a_beat_at_the_leader_tempo() {
    // a quarter of a beat, and a length beats do not divide
    for beats in [0.25, 0.3] {
        let mut rig = locked_follower();
        let start = rig.deck(1).position_s;
        let region = LoopRegion { start_s: start, end_s: start + beats * 60.0 / 125.0 };
        rig.send(Command::SetLoop { deck: 1, region: Some(region), active: true });
        let mut elapsed = 0.0;
        for _ in 0..40 {
            elapsed += rig.run(0.05);
            let expected = region.wrap(start + elapsed * LEADER_BPM / 125.0);
            assert_close(rig.deck(1).position_s, expected, 1e-3);
        }
    }

    let mut rig = locked_follower();
    rig.send(Command::LoopRoll { deck: 1, beats: Some(0.25) });
    rig.run(0.05);
    let region = rig.deck(1).looping.region.unwrap();
    for _ in 0..40 {
        rig.run(0.05);
        let position = rig.deck(1).position_s;
        assert!(region.contains(position), "{position} out of {region:?}");
    }
    rig.send(Command::LoopRoll { deck: 1, beats: None });
    rig.run(0.05);
    assert!(phase_gap(&mut rig).abs() < 0.01, "{}", phase_gap(&mut rig));
}

#[test]
fn a_follower_jumped_out_of_its_loop_keeps_to_the_leader_s_beats_again() {
    let mut rig = locked_follower();
    rig.send(Command::BeatLoop { deck: 1, beats: 1.0 });
    rig.run(1.0);
    // a cue a third of a beat off, out of the loop, which stays on
    rig.send(Command::SetQuantize { enabled: false });
    let cue = rig.deck(1).position_s + (20.0 + 1.0 / 3.0) * 60.0 / 125.0;
    rig.send(Command::Jump { deck: 1, seconds: cue });
    rig.run(0.5);
    assert!(rig.deck(1).looping.active);
    assert!(phase_gap(&mut rig).abs() < 0.01, "{}", phase_gap(&mut rig));
}

#[test]
fn sync_engaged_in_a_loop_shorter_than_a_beat_stays_in_it() {
    let mut rig = leader_with_follower(125.0);
    play_internal(&mut rig, 1, 10.07);
    rig.run(0.1);
    let start = rig.deck(1).position_s;
    let region = LoopRegion { start_s: start, end_s: start + 0.25 * 60.0 / 125.0 };
    rig.send(Command::SetLoop { deck: 1, region: Some(region), active: true });
    rig.send(Command::SetSyncLocked { deck: 1, locked: true });
    for _ in 0..20 {
        rig.run(0.05);
        let position = rig.deck(1).position_s;
        assert!(region.contains(position), "{position} out of {region:?}");
    }
}

#[test]
fn tempos_an_octave_apart_play_at_their_own_speed() {
    let mut rig = leader_with_follower(LEADER_BPM / 2.0);
    play_internal(&mut rig, 1, 5.0);
    rig.send(Command::SetSyncLocked { deck: 1, locked: true });
    rig.run(0.5);
    assert_close(mean_speed(&mut rig, 1, 1.0), 1.0, 1e-3);
}

#[test]
fn a_follower_keeps_its_tempo_when_the_leader_stops() {
    let mut rig = leader_with_follower(125.0);
    play_internal(&mut rig, 1, 10.0);
    rig.send(Command::SetSyncLocked { deck: 1, locked: true });
    rig.run(1.0);
    rig.send(Command::SetPlaying { deck: 0, playing: false });
    rig.run(1.0);
    assert_close(mean_speed(&mut rig, 1, 1.0), LEADER_BPM / 125.0, 1e-3);
}

#[test]
fn relative_one_shot_sync_matches_once_then_the_pitch_fader_works() {
    let mut rig = leader_with_follower(124.0);
    relative_record(&mut rig, 1, 1.03);
    rig.run(1.0);

    rig.send(Command::SyncOnce { deck: 1 });
    rig.run(0.2);
    let follower = rig.deck(1);
    assert!(!follower.sync.locked);
    let matched = LEADER_BPM / 124.0;
    assert!((follower.sync.rate_offset - matched / 1.03).abs() < 0.003, "{follower:?}");
    assert!(phase_gap(&mut rig).abs() < 0.03, "{}", phase_gap(&mut rig));
    assert_close(mean_speed(&mut rig, 1, 1.0), matched, 0.003);

    rig.set_turntable_speed(1, 1.06);
    rig.run(0.5);
    assert_close(mean_speed(&mut rig, 1, 1.0), 1.06 * follower.sync.rate_offset, 0.003);
}

#[test]
fn relative_locked_sync_ignores_the_pitch_fader_but_not_the_gestures() {
    let mut rig = leader_with_follower(124.0);
    relative_record(&mut rig, 1, 1.03);
    rig.run(1.0);
    rig.send(Command::SetSyncLocked { deck: 1, locked: true });
    rig.run(0.5);
    let matched = LEADER_BPM / 124.0;
    assert_close(mean_speed(&mut rig, 1, 1.0), matched, 0.003);

    // a pitch fader move first acts like a nudge, then is absorbed
    rig.set_turntable_speed(1, 1.06);
    rig.run(0.1);
    assert!(mean_speed(&mut rig, 1, 0.2) > matched * 1.015);
    rig.run(8.0);
    assert_close(mean_speed(&mut rig, 1, 1.0), matched, 0.003);
    assert!(rig.deck(1).sync.locked);
}

#[test]
fn a_locked_record_keeps_the_tempo_when_the_leader_stops() {
    let mut rig = leader_with_follower(124.0);
    relative_record(&mut rig, 1, 1.03);
    rig.run(1.0);
    rig.send(Command::SetSyncLocked { deck: 1, locked: true });
    rig.run(0.5);
    rig.send(Command::SetPlaying { deck: 0, playing: false });
    rig.run(1.0);
    assert_close(mean_speed(&mut rig, 1, 1.0), LEADER_BPM / 124.0, 0.003);
}

#[test]
fn absolute_mode_has_no_sync() {
    let mut rig = leader_with_follower(124.0);
    rig.turntable(1, 20.0, 1.0);
    rig.send(Command::SetVinylMode { deck: 1, mode: VinylMode::Absolute });
    rig.send(Command::SetVinylEnabled { deck: 1, enabled: true });
    rig.send(Command::SetSyncLocked { deck: 1, locked: true });
    rig.send(Command::SyncOnce { deck: 1 });
    rig.run(0.5);
    let follower = rig.deck(1);
    assert!(!follower.sync.locked);
    assert_eq!(follower.sync.rate_offset, 1.0);
}

#[test]
fn the_chosen_leader_leads_and_a_locked_deck_never_does() {
    let mut rig = leader_with_follower(125.0);
    play_internal(&mut rig, 1, 10.0);
    rig.send(Command::SetSyncLeader { deck: Some(1) });
    rig.send(Command::SetSyncLocked { deck: 0, locked: true });
    rig.run(0.5);
    assert!(rig.deck(1).sync.leader && !rig.deck(0).sync.leader);
    assert_close(mean_speed(&mut rig, 0, 1.0), 125.0 / LEADER_BPM, 1e-3);

    rig.send(Command::SetSyncLocked { deck: 1, locked: true });
    rig.run(0.2);
    assert!(!rig.deck(1).sync.leader);
}

/// An engine started again, both decks playing on their own: deck 0 first in order.
fn restarted() -> Rig {
    let mut rig = Rig::new();
    load(&mut rig, 0, 120.0, 0.0);
    load(&mut rig, 1, 150.0, 0.0);
    play_internal(&mut rig, 0, 10.0);
    play_internal(&mut rig, 1, 10.0);
    rig
}

#[test]
fn the_leader_resumed_after_a_restart_leads_again_rather_than_the_first_deck() {
    let mut rig = restarted();
    rig.send(Command::ResumeSyncLeader { deck: 1 });
    for _ in 0..100 {
        rig.run(0.01);
        assert!(rig.deck(1).sync.leader && !rig.deck(0).sync.leader);
    }
}

#[test]
fn a_follower_keeps_to_the_resumed_leader_from_the_restart_on() {
    let mut rig = restarted();
    rig.send(Command::SetSyncLocked { deck: 0, locked: true });
    rig.send(Command::ResumeSyncLeader { deck: 1 });
    rig.run(0.01);
    assert_close(mean_speed(&mut rig, 0, 0.05), 150.0 / 120.0, 0.02);
}

#[test]
fn a_resumed_leader_that_never_gets_up_to_speed_gives_the_lead_up() {
    let mut rig = restarted();
    rig.send(Command::SetPlaying { deck: 1, playing: false });
    rig.send(Command::ResumeSyncLeader { deck: 1 });
    rig.run(0.5);
    assert!(!rig.deck(0).sync.leader);
    rig.run(1.0);
    assert!(rig.deck(0).sync.leader && !rig.deck(1).sync.leader);
}

#[test]
fn a_new_track_drops_the_sync() {
    let mut rig = leader_with_follower(125.0);
    rig.send(Command::SetSyncLocked { deck: 1, locked: true });
    rig.run(0.1);
    load(&mut rig, 1, 130.0, 0.0);
    rig.run(0.1);
    assert!(!rig.deck(1).sync.locked);
}
