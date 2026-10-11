//! Where the decks are heard: an external mixer or the software mixer with headphones.

mod common;

use common::{constant_track, Rig};
use prisme_engine::{Command, CrossfaderSide, MixMode, OutputRouting};

/// Deck 1 plays a constant 0.5, deck 2 a constant 0.25, on a 4-output device.
fn two_playing_decks(mode: MixMode) -> Rig {
    let mut rig = Rig::with_outputs(4);
    rig.send(Command::SetRouting { routing: OutputRouting { mode, ..OutputRouting::default() } });
    for (deck, level) in [(0, 0.5), (1, 0.25)] {
        rig.send(Command::LoadTrack { deck, track: constant_track(10.0, level) });
        rig.send(Command::Seek { deck, seconds: 1.0 });
        rig.send(Command::SetPlaying { deck, playing: true });
    }
    rig.send(Command::SetCrossfaderSide { deck: 0, side: CrossfaderSide::Left });
    rig.send(Command::SetCrossfaderSide { deck: 1, side: CrossfaderSide::Right });
    rig
}

#[track_caller]
fn assert_frame(actual: &[f32], expected: [f32; 4]) {
    let close = actual.iter().zip(expected).all(|(a, e)| (a - e).abs() < 1e-4);
    assert!(close, "{actual:?} instead of {expected:?}");
}

#[test]
fn with_an_external_mixer_each_deck_has_its_outputs_untouched() {
    let mut rig = two_playing_decks(MixMode::External);
    rig.send(Command::SetCrossfader { position: -1.0 });
    rig.send(Command::SetVolume { deck: 0, gain: 0.2 });
    rig.run(0.05);
    assert_frame(rig.output_frame(), [0.5, 0.5, 0.25, 0.25]);
}

#[test]
fn the_software_mixer_mixes_the_master_and_feeds_the_headphones() {
    let mut rig = two_playing_decks(MixMode::Internal);
    rig.send(Command::SetCrossfader { position: -1.0 });
    rig.send(Command::SetDeckCue { deck: 1, enabled: true });
    rig.run(0.05);
    // deck 2 is faded out of the master but cued in the headphones
    assert_frame(rig.output_frame(), [0.5, 0.5, 0.25, 0.25]);

    rig.send(Command::SetCueMix { mix: 1.0 });
    rig.send(Command::SetHeadphoneGain { gain: 0.5 });
    rig.run(0.05);
    assert_frame(rig.output_frame(), [0.5, 0.5, 0.25, 0.25]);

    rig.send(Command::SetCrossfader { position: 0.0 });
    rig.send(Command::SetDeckCue { deck: 1, enabled: false });
    rig.send(Command::SetCueMix { mix: 0.0 });
    rig.run(0.05);
    let half = std::f32::consts::FRAC_1_SQRT_2;
    let master = 0.5 * half + 0.25 * half;
    assert_frame(rig.output_frame(), [master, master, 0.0, 0.0]);
}

#[test]
fn each_deck_reports_its_level() {
    let mut rig = two_playing_decks(MixMode::External);
    rig.run(0.05);
    assert!((rig.deck(0).peak - 0.5).abs() < 1e-4);
    assert!((rig.deck(1).peak - 0.25).abs() < 1e-4);
}

#[test]
fn a_stopped_deck_is_silent_not_frozen_on_its_last_sample() {
    let mut rig = two_playing_decks(MixMode::External);
    rig.run(0.05);
    assert_frame(rig.output_frame(), [0.5, 0.5, 0.25, 0.25]);
    rig.send(Command::SetPlaying { deck: 0, playing: false });
    rig.run(0.05);
    assert_frame(rig.output_frame(), [0.0, 0.0, 0.25, 0.25]);
}
