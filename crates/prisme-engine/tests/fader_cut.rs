//! Fader cuts: the deck chopped on its beats while a pad is held.

mod common;

use common::{constant_track, Rig};
use prisme_engine::{BeatGrid, Command};

/// A deck playing a constant level on a 120 BPM grid, from a beat.
fn playing() -> Rig {
    let mut rig = Rig::new();
    rig.send(Command::SetLimiter { enabled: false });
    rig.send(Command::LoadTrack { deck: 0, track: constant_track(60.0, 0.5) });
    rig.send(Command::SetBeatGrid {
        deck: 0,
        grid: Some(BeatGrid { bpm: 120.0, first_beat_s: 0.0, bars_sure: true }),
    });
    rig.send(Command::Seek { deck: 0, seconds: 10.0 });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.run(0.1);
    rig
}

#[test]
fn a_cut_held_lets_half_the_time_through_then_all_of_it_once_released() {
    for beats in [1.0 / 16.0, 0.125, 0.25, 0.5] {
        let mut rig = playing();
        let open = rig.run_rms(1.0);
        rig.send(Command::FaderCut { deck: 0, beats: Some(beats) });
        let cut = rig.run_rms(1.0);
        // half the time at full level: the power halves
        let ratio = cut / open;
        assert!((ratio - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.03, "{beats} beat: {ratio}");
        rig.send(Command::FaderCut { deck: 0, beats: None });
        rig.run(0.01);
        assert!((rig.run_rms(0.5) / open - 1.0).abs() < 1e-3);
    }
}

#[test]
fn a_cut_follows_the_grid() {
    let mut rig = playing();
    rig.send(Command::FaderCut { deck: 0, beats: Some(0.5) });
    // a quarter of a beat into a half-beat cut: closed until the next half beat
    rig.send(Command::Seek { deck: 0, seconds: 20.15 });
    rig.run(0.02);
    assert!(rig.output_frame()[0].abs() < 1e-3, "{}", rig.output_frame()[0]);
    rig.send(Command::Seek { deck: 0, seconds: 20.0 });
    rig.run(0.02);
    assert!(rig.output_frame()[0] > 0.1, "open from the beat");
}
