//! The master's balance, the booth, the live inputs (microphone, aux line) with talkover, and
//! decks passing their input through.

mod common;

use common::{constant_track, Rig};
use prisme_engine::{Command, InputChannels, LiveSource, MixMode, OutputRouting};

#[track_caller]
fn assert_frame(actual: &[f32], expected: &[f32]) {
    let close = actual.iter().zip(expected).all(|(a, e)| (a - e).abs() < 1e-4);
    assert!(close && actual.len() == expected.len(), "{actual:?} instead of {expected:?}");
}

/// Deck 1 plays a constant `level` through the software mixer, master on 1-2, headphones on
/// 3-4, booth on 5-6.
fn deck_playing(rig: &mut Rig, level: f32) {
    let routing = OutputRouting { booth: Some(4), ..OutputRouting::default() };
    rig.send(Command::SetRouting { routing });
    rig.send(Command::LoadTrack { deck: 0, track: constant_track(10.0, level) });
    rig.send(Command::Seek { deck: 0, seconds: 1.0 });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
}

fn mic_on(rig: &mut Rig, first: usize, gain: f32) {
    let channels = Some(InputChannels { first, stereo: false });
    rig.send(Command::SetLiveInput { source: LiveSource::Mic, channels });
    rig.send(Command::SetLiveLevel { source: LiveSource::Mic, gain });
    rig.send(Command::SetLiveOn { source: LiveSource::Mic, on: true });
}

#[test]
fn the_balance_and_the_booth_level_go_to_their_outputs() {
    let mut rig = Rig::with_outputs(6);
    deck_playing(&mut rig, 0.5);
    rig.run(0.1);
    assert_frame(rig.output_frame(), &[0.5, 0.5, 0.0, 0.0, 0.5, 0.5]);

    rig.send(Command::SetBalance { balance: 0.5 });
    rig.send(Command::SetBoothGain { gain: 0.4 });
    rig.run(0.2);
    assert_frame(rig.output_frame(), &[0.25, 0.5, 0.0, 0.0, 0.2, 0.2]);

    rig.send(Command::SetBalance { balance: -1.0 });
    rig.send(Command::SetCueMix { mix: 1.0 });
    rig.run(0.2);
    assert_frame(rig.output_frame(), &[0.5, 0.0, 0.5, 0.5, 0.2, 0.2]);
}

#[test]
fn the_microphone_and_the_aux_line_are_mixed_in_at_their_levels() {
    // the turntables on inputs 1-4, the microphone on 5, the aux line on 7-8
    let mut rig = Rig::with_channels(8, 4);
    rig.set_input_level(4, 0.2);
    rig.set_input_level(6, 0.4);
    rig.set_input_level(7, 0.8);
    mic_on(&mut rig, 4, 0.5);
    let aux = Some(InputChannels { first: 6, stereo: true });
    rig.send(Command::SetLiveInput { source: LiveSource::Aux, channels: aux });
    rig.send(Command::SetLiveLevel { source: LiveSource::Aux, gain: 0.25 });
    rig.send(Command::SetLiveOn { source: LiveSource::Aux, on: true });
    rig.send(Command::SetCueMix { mix: 1.0 });
    rig.run(0.2);
    // the headphones on the master hear the music without the microphone
    assert_frame(rig.output_frame(), &[0.2, 0.3, 0.1, 0.2]);
    let status = rig.controller.status();
    assert!((status.mic_peak - 0.1).abs() < 1e-4 && (status.aux_peak - 0.2).abs() < 1e-4);

    rig.send(Command::SetCueMix { mix: 0.0 });
    rig.send(Command::SetLiveCue { source: LiveSource::Mic, cue: true });
    rig.send(Command::SetLiveOn { source: LiveSource::Aux, on: false });
    rig.run(0.2);
    assert_frame(rig.output_frame(), &[0.1, 0.1, 0.1, 0.1]);

    rig.send(Command::SetLiveOn { source: LiveSource::Mic, on: false });
    rig.run(0.2);
    assert_frame(rig.output_frame(), &[0.0, 0.0, 0.1, 0.1]);
}

#[test]
fn talkover_ducks_the_music_while_the_microphone_is_on_and_brings_it_back() {
    let mut rig = Rig::with_channels(6, 6);
    deck_playing(&mut rig, 0.5);
    rig.send(Command::SetTalkover { enabled: true, depth_db: -20.0 });
    rig.run(0.1);
    mic_on(&mut rig, 4, 1.0);
    rig.run(0.01);
    assert!(rig.output_frame()[0] > 0.4, "it glides down");
    rig.run(0.5);
    assert_frame(rig.output_frame(), &[0.05, 0.05, 0.0, 0.0, 0.05, 0.05]);

    rig.send(Command::SetLiveOn { source: LiveSource::Mic, on: false });
    rig.run(0.01);
    assert!(rig.output_frame()[0] < 0.2, "it glides back");
    rig.run(1.5);
    assert_frame(rig.output_frame(), &[0.5, 0.5, 0.0, 0.0, 0.5, 0.5]);

    rig.send(Command::SetTalkover { enabled: false, depth_db: -20.0 });
    rig.send(Command::SetLiveOn { source: LiveSource::Mic, on: true });
    rig.run(0.5);
    assert_frame(rig.output_frame(), &[0.5, 0.5, 0.0, 0.0, 0.5, 0.5]);
}

#[test]
fn a_deck_in_passthrough_plays_its_input_instead_of_its_track() {
    for mode in [MixMode::Internal, MixMode::External] {
        let mut rig = Rig::with_outputs(4);
        deck_playing(&mut rig, 0.5);
        rig.send(Command::SetRouting { routing: OutputRouting { mode, ..OutputRouting::default() } });
        rig.set_input_level(0, 0.3);
        rig.set_input_level(1, -0.3);
        rig.run(0.1);
        assert_frame(rig.output_frame(), &[0.5, 0.5, 0.0, 0.0]);

        rig.send(Command::SetPassthrough { deck: 0, enabled: true });
        rig.run(0.05);
        let waiting_at = rig.deck(0).position_s;
        rig.run(0.5);
        assert_frame(rig.output_frame(), &[0.3, -0.3, 0.0, 0.0]);
        assert_eq!(rig.deck(0).position_s, waiting_at, "the track waits");

        rig.send(Command::SetPassthrough { deck: 0, enabled: false });
        rig.run(0.1);
        assert_frame(rig.output_frame(), &[0.5, 0.5, 0.0, 0.0]);
        assert!(rig.deck(0).position_s > waiting_at, "and goes on from there");
    }
}

#[test]
fn passthrough_goes_through_the_channel_and_its_fader() {
    let mut rig = Rig::with_outputs(2);
    deck_playing(&mut rig, 0.5);
    rig.set_input_level(0, 0.4);
    rig.set_input_level(1, 0.4);
    rig.send(Command::SetPassthrough { deck: 0, enabled: true });
    rig.send(Command::SetVolume { deck: 0, gain: 0.5 });
    rig.run(0.2);
    assert_frame(rig.output_frame(), &[0.2, 0.2]);
}
