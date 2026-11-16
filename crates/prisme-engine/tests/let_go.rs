//! A record letting go of its deck (vinyl control turned off, or the turntable given to the
//! deck layered with it): the deck plays on by itself as the record played it.

mod common;

use common::{silent_track, Rig};
use prisme_engine::{Command, VinylMode};

/// A record turning at `speed` in `mode` in the middle of the track, long enough for its tempo
/// to be known.
fn record(mode: VinylMode, speed: f64) -> Rig {
    let mut rig = Rig::new();
    rig.turntable(0, 120.0, speed);
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(300.0) });
    rig.send(Command::Seek { deck: 0, seconds: 120.0 });
    rig.send(Command::SetPitch { deck: 0, ratio: 0.95 });
    rig.send(Command::SetVinylMode { deck: 0, mode });
    rig.send(Command::SetVinylEnabled { deck: 0, enabled: true });
    rig.run(5.0);
    rig
}

fn let_go(rig: &mut Rig) {
    rig.send(Command::SetVinylEnabled { deck: 0, enabled: false });
    rig.turntables[0] = None;
    rig.run(1.0);
}

#[test]
fn the_deck_plays_on_at_the_record_tempo_not_its_fader() {
    for mode in [VinylMode::Relative, VinylMode::Absolute] {
        let mut rig = record(mode, 1.03);
        let before = rig.deck(0).position_s;
        let_go(&mut rig);
        let d = rig.deck(0);
        assert!(d.playing && !d.reverse, "{mode:?} {d:?}");
        assert!((d.rate - 1.03).abs() < 0.002, "{mode:?} {d:?}");
        assert!((d.speed - 1.03).abs() < 0.002, "{mode:?} {d:?}");
        assert!((d.position_s - before - 1.03).abs() < 0.05, "{mode:?} {d:?}");
    }
}

#[test]
fn a_record_played_backwards_leaves_the_deck_in_reverse() {
    let mut rig = record(VinylMode::Relative, -1.0);
    let_go(&mut rig);
    let d = rig.deck(0);
    assert!(d.playing && d.reverse, "{d:?}");
    assert!((d.speed + 1.0).abs() < 0.01, "{d:?}");
}

#[test]
fn a_record_standing_still_leaves_the_deck_stopped() {
    let mut rig = record(VinylMode::Absolute, 1.0);
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.set_turntable_speed(0, 0.0);
    rig.run(1.0);
    let before = rig.deck(0).position_s;
    let_go(&mut rig);
    let d = rig.deck(0);
    assert!(!d.playing, "{d:?}");
    assert_eq!(d.position_s, before);
}

#[test]
fn a_deck_playing_by_itself_is_left_as_it_is() {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(300.0) });
    rig.send(Command::SetPitch { deck: 0, ratio: 0.95 });
    let_go(&mut rig);
    let d = rig.deck(0);
    assert!(!d.playing, "{d:?}");
    assert!((d.rate - 0.95).abs() < 1e-12, "{d:?}");
}
