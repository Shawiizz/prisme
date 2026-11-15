//! The preview: a track of the library heard in the headphones alone, never by the audience.

mod common;

use common::{constant_track, Rig};
use prisme_engine::{Command, MixMode, OutputRouting, RecordSource};

fn rig(outputs: usize, mode: MixMode, headphones: Option<usize>) -> Rig {
    let mut rig = Rig::with_outputs(outputs);
    rig.send(Command::SetRouting { routing: OutputRouting { mode, headphones, ..OutputRouting::default() } });
    rig
}

fn preview(rig: &mut Rig, id: u64, level: f32, start_s: f64) {
    rig.send(Command::Preview { id, track: constant_track(10.0, level), start_s });
}

#[track_caller]
fn assert_frame(actual: &[f32], expected: &[f32]) {
    let close = actual.iter().zip(expected).all(|(a, e)| (a - e).abs() < 1e-5);
    assert!(close && actual.len() == expected.len(), "{actual:?} instead of {expected:?}");
}

#[test]
fn the_software_mixer_plays_it_in_the_headphones_alone_at_their_level() {
    let mut rig = rig(4, MixMode::Internal, Some(2));
    let (sink, mut recorded) = rtrb::RingBuffer::new(1 << 16);
    rig.send(Command::StartRecording { source: RecordSource::Master, sink });
    preview(&mut rig, 1, 0.5, 3.0);
    rig.run(0.05);
    assert_frame(rig.output_frame(), &[0.0, 0.0, 0.5, 0.5]);
    assert_eq!(rig.controller.status().main_peak, 0.0, "not on the main meter");
    let recorded: Vec<f32> = std::iter::from_fn(|| recorded.pop().ok()).collect();
    assert!(!recorded.is_empty() && recorded.iter().all(|s| *s == 0.0), "not recorded");

    // whatever the headphones mix, at their level
    rig.send(Command::SetCueMix { mix: 1.0 });
    rig.send(Command::SetHeadphoneGain { gain: 0.5 });
    rig.run(0.05);
    assert_frame(rig.output_frame(), &[0.0, 0.0, 0.25, 0.25]);
}

#[test]
fn without_headphones_it_is_heard_nowhere() {
    let mut rig = rig(2, MixMode::Internal, None);
    preview(&mut rig, 1, 0.5, 0.0);
    rig.run(0.05);
    assert_frame(rig.output_frame(), &[0.0, 0.0]);
}

#[test]
fn with_an_external_mixer_it_takes_a_pair_no_deck_uses() {
    let mut rig = rig(6, MixMode::External, Some(4));
    rig.send(Command::LoadTrack { deck: 0, track: constant_track(10.0, 0.25) });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    preview(&mut rig, 1, 0.5, 0.0);
    rig.run(0.05);
    assert_frame(rig.output_frame(), &[0.25, 0.25, 0.0, 0.0, 0.5, 0.5]);
}

#[test]
fn on_a_deck_pair_of_an_external_mixer_it_is_heard_nowhere() {
    let mut rig = rig(4, MixMode::External, Some(2));
    preview(&mut rig, 1, 0.5, 0.0);
    rig.run(0.05);
    assert_frame(rig.output_frame(), &[0.0; 4]);
}

#[test]
fn it_starts_where_asked_and_seeks() {
    let mut rig = rig(4, MixMode::Internal, Some(2));
    preview(&mut rig, 7, 0.5, 3.0);
    let ran = rig.run(0.1);
    let status = rig.controller.status().preview.expect("previewing");
    assert_eq!(status.id, 7);
    assert!((status.position_s - (3.0 + ran)).abs() < 1e-3, "{}", status.position_s);
    assert!(!status.waiting);

    rig.send(Command::SeekPreview { seconds: 8.0 });
    let ran = rig.run(0.1);
    assert!((rig.controller.status().preview.unwrap().position_s - (8.0 + ran)).abs() < 1e-3);
    rig.send(Command::SkipPreview { seconds: -5.0 });
    let back = rig.run(0.1);
    let expected = 8.0 + ran - 5.0 + back;
    assert!((rig.controller.status().preview.unwrap().position_s - expected).abs() < 1e-3);
}

#[test]
fn another_track_replaces_it_and_stop_stops_it() {
    let mut rig = rig(4, MixMode::Internal, Some(2));
    preview(&mut rig, 1, 0.5, 0.0);
    rig.run(0.05);
    preview(&mut rig, 2, 0.25, 0.0);
    rig.run(0.05);
    assert_frame(rig.output_frame(), &[0.0, 0.0, 0.25, 0.25]);
    assert_eq!(rig.controller.status().preview.map(|p| p.id), Some(2));
    assert_eq!(rig.controller.collect_garbage(), 1, "the first track is handed back");

    rig.send(Command::StopPreview);
    rig.run(0.05);
    assert_frame(rig.output_frame(), &[0.0; 4]);
    assert!(rig.controller.status().preview.is_none());
    assert_eq!(rig.controller.collect_garbage(), 1);
}

#[test]
fn it_ends_with_its_track() {
    let mut rig = rig(4, MixMode::Internal, Some(2));
    preview(&mut rig, 1, 0.5, 9.95);
    rig.run(0.1);
    assert!(rig.controller.status().preview.is_none());
    assert_eq!(rig.controller.collect_garbage(), 1);
}
