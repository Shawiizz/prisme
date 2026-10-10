//! A test rig: the engine fed by synthetic turntables, block by block.

#![allow(dead_code)]

use prisme_engine::{create, Command, DeckStatus, Engine, EngineConfig, EngineController};
use prisme_media::TrackAudio;
use prisme_timecode::testing::SignalGenerator;
use prisme_timecode::SERATO_CV02_A;
use std::sync::Arc;

pub const SR: u32 = 48_000;
pub const BLOCK: usize = 256;
const DECKS: usize = 2;

pub fn silent_track(seconds: f64) -> Arc<TrackAudio> {
    Arc::new(TrackAudio::from_interleaved(44_100, vec![0.0; (seconds * 44_100.0) as usize * 2]))
}

/// A record on a turntable, turning at `speed`.
pub struct Turntable {
    pub generator: SignalGenerator,
    pub speed: f64,
}

pub struct Rig {
    engine: Engine,
    pub controller: EngineController,
    pub turntables: [Option<Turntable>; DECKS],
    deck_signal: Vec<f32>,
    input: Vec<f32>,
    output: Vec<f32>,
}

impl Rig {
    pub fn new() -> Self {
        // a snapshot after every block, so that positions read by the tests are exact
        let config =
            EngineConfig { status_rate_hz: f64::INFINITY, ..EngineConfig::new(SR, DECKS, &SERATO_CV02_A) };
        let (engine, controller) = create(config);
        Self {
            engine,
            controller,
            turntables: [None, None],
            deck_signal: Vec::new(),
            input: vec![0.0; BLOCK * DECKS * 2],
            output: vec![0.0; BLOCK * 2],
        }
    }

    /// Puts a record on `deck`'s turntable at `start_s` on the record.
    pub fn turntable(&mut self, deck: usize, start_s: f64, speed: f64) {
        let generator = SignalGenerator::new(&SERATO_CV02_A, SR, start_s * 1000.0, 600_000.0);
        self.turntables[deck] = Some(Turntable { generator, speed });
    }

    pub fn set_turntable_speed(&mut self, deck: usize, speed: f64) {
        self.turntables[deck].as_mut().expect("a turntable").speed = speed;
    }

    pub fn send(&mut self, c: Command) {
        assert!(self.controller.send(c).is_ok());
    }

    /// Runs whole blocks for about `seconds`; returns the exact time run.
    pub fn run(&mut self, seconds: f64) -> f64 {
        let blocks = (seconds * SR as f64 / BLOCK as f64).round() as usize;
        let channels = DECKS * 2;
        for _ in 0..blocks {
            self.input.fill(0.0);
            for (deck, turntable) in self.turntables.iter_mut().enumerate() {
                let Some(t) = turntable else { continue };
                self.deck_signal.clear();
                let speed = t.speed;
                t.generator.render(BLOCK, |_| speed, &mut self.deck_signal);
                for (frame, lr) in self.input.chunks_exact_mut(channels).zip(self.deck_signal.chunks_exact(2))
                {
                    frame[deck * 2] = lr[0];
                    frame[deck * 2 + 1] = lr[1];
                }
            }
            self.engine.process(&self.input, channels, &mut self.output, 2);
        }
        (blocks * BLOCK) as f64 / SR as f64
    }

    pub fn deck(&mut self, deck: usize) -> DeckStatus {
        self.controller.status().decks[deck]
    }
}
