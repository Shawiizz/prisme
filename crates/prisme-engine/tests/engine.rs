//! End-to-end behaviour of the engine, with a synthetic turntable on the input.

use prisme_engine::{create, Command, EngineConfig, EngineController, VinylMode};
use prisme_media::TrackAudio;
use prisme_timecode::testing::SignalGenerator;
use prisme_timecode::SERATO_CV02_A;
use std::sync::Arc;

const SR: u32 = 48_000;
const BLOCK: usize = 256;

fn silent_track(seconds: f64) -> Arc<TrackAudio> {
    Arc::new(TrackAudio::from_interleaved(44_100, vec![0.0; (seconds * 44_100.0) as usize * 2]))
}

struct Rig {
    engine: prisme_engine::Engine,
    controller: EngineController,
    turntable: Option<(SignalGenerator, f64)>,
    input: Vec<f32>,
    output: Vec<f32>,
}

impl Rig {
    fn new() -> Self {
        let (engine, controller) = create(EngineConfig::new(SR, 2, &SERATO_CV02_A));
        Self { engine, controller, turntable: None, input: Vec::new(), output: vec![0.0; BLOCK * 2] }
    }

    /// Puts a record on deck 1's turntable at `start_s` on the record, turning at `speed`.
    fn turntable(&mut self, start_s: f64, speed: f64) {
        self.turntable = Some((SignalGenerator::new(&SERATO_CV02_A, SR, start_s * 1000.0, 120_000.0), speed));
    }

    fn send(&mut self, c: Command) {
        assert!(self.controller.send(c).is_ok());
    }

    fn run(&mut self, seconds: f64) {
        let blocks = (seconds * SR as f64 / BLOCK as f64).round() as usize;
        for _ in 0..blocks {
            self.input.clear();
            match self.turntable.as_mut() {
                Some((generator, speed)) => {
                    let s = *speed;
                    generator.render(BLOCK, |_| s, &mut self.input);
                }
                None => self.input.resize(BLOCK * 2, 0.0),
            }
            self.engine.process(&self.input, 2, &mut self.output, 2);
        }
    }
}

#[test]
fn internal_play_follows_the_pitch() {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(10.0) });
    rig.send(Command::SetPitch { deck: 0, ratio: 1.08 });
    rig.send(Command::SetPlaying { deck: 0, playing: true });
    rig.run(1.0);
    let d = rig.controller.status().decks[0];
    assert!(d.loaded && d.playing);
    assert!((d.position_s - 1.08).abs() < 0.02, "{d:?}");
    assert!((d.speed - 1.08).abs() < 1e-9);
}

#[test]
fn relative_vinyl_follows_the_record_speed() {
    let mut rig = Rig::new();
    rig.turntable(20.0, 1.05);
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(30.0) });
    rig.send(Command::SetVinylMode { deck: 0, mode: VinylMode::Relative });
    rig.send(Command::SetVinylEnabled { deck: 0, enabled: true });
    rig.run(2.0);
    let d = rig.controller.status().decks[0];
    assert!((d.speed - 1.05).abs() < 0.01, "{d:?}");
    // relative mode ignores the absolute position: the track started at 0
    assert!(d.position_s > 1.9 && d.position_s < 2.2, "{d:?}");
}

#[test]
fn absolute_vinyl_follows_the_record_position() {
    let mut rig = Rig::new();
    rig.turntable(12.0, 1.0);
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(40.0) });
    rig.send(Command::SetLeadIn { deck: 0, seconds: 2.0 });
    rig.send(Command::SetVinylEnabled { deck: 0, enabled: true });
    rig.run(1.0);
    let d = rig.controller.status().decks[0];
    // record at 13 s, minus 2 s of lead-in
    assert!((d.position_s - 11.0).abs() < 0.02, "{d:?}");
    let record = d.vinyl.record_position_s.expect("locked");
    assert!((record - 13.0).abs() < 0.02, "{d:?}");

    // needle drop 15 s further
    rig.turntable.as_mut().unwrap().0.jump_ms(15_000.0);
    rig.run(0.5);
    let d = rig.controller.status().decks[0];
    assert!((d.position_s - 26.5).abs() < 0.05, "{d:?}");
}

#[test]
fn replaced_tracks_are_freed_on_the_control_side() {
    let mut rig = Rig::new();
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(1.0) });
    rig.send(Command::LoadTrack { deck: 0, track: silent_track(1.0) });
    rig.send(Command::Eject { deck: 0 });
    rig.send(Command::LoadTrack { deck: 7, track: silent_track(1.0) }); // no such deck
    rig.run(0.01);
    assert_eq!(rig.controller.collect_garbage(), 3);
}
