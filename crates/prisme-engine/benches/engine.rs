//! Cost of one audio callback with two decks playing (one from the internal transport at
//! +4 % with the key lock, one driven by a turntable in relative mode), through the
//! software mixer, for the buffer sizes of a low-latency interface. Budget: a callback in under 25 % of its own duration with everything on.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use prisme_engine::{create, Command, EngineConfig, VinylMode};
use prisme_media::TrackAudio;
use prisme_timecode::testing::SignalGenerator;
use prisme_timecode::SERATO_CV02_A;
use std::sync::Arc;

const RATE: u32 = 48_000;
const INPUT_CHANNELS: usize = 4;
const OUTPUT_CHANNELS: usize = 4;

fn track(seconds: usize) -> Arc<TrackAudio> {
    let frames = seconds * RATE as usize;
    let samples = (0..frames * 2).map(|i| ((i as f32) * 0.001).sin() * 0.5).collect();
    Arc::new(TrackAudio::from_interleaved(RATE, samples))
}

fn engine(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine callback");
    for frames in [64usize, 256] {
        let (mut engine, mut controller) = create(EngineConfig::new(RATE, 2, &SERATO_CV02_A));
        for command in [
            Command::LoadTrack { deck: 0, track: track(60) },
            Command::LoadTrack { deck: 1, track: track(60) },
            Command::SetPlaying { deck: 0, playing: true },
            Command::SetPitch { deck: 0, ratio: 1.04 },
            Command::SetKeyLock { deck: 0, enabled: true },
            Command::SetVinylEnabled { deck: 1, enabled: true },
            Command::SetVinylMode { deck: 1, mode: VinylMode::Relative },
        ] {
            assert!(controller.send(command).is_ok(), "command queue full");
        }
        // turntable on inputs 3-4, a second of signal played in a loop
        let mut timecode = Vec::new();
        SignalGenerator::new(&SERATO_CV02_A, RATE, 30_000.0, 2_000.0).render(
            RATE as usize,
            |_| 1.0,
            &mut timecode,
        );
        let blocks: Vec<Vec<f32>> = timecode
            .chunks(2 * frames)
            .map(|tc| tc.chunks(2).flat_map(|f| [0.0, 0.0, f[0], f[1]]).collect())
            .collect();
        let mut output = vec![0.0; frames * OUTPUT_CHANNELS];
        let mut next = 0;
        group.throughput(Throughput::Elements(frames as u64));
        group.bench_function(BenchmarkId::new("2 decks", frames), |b| {
            b.iter(|| {
                engine.process(&blocks[next], INPUT_CHANNELS, &mut output, OUTPUT_CHANNELS);
                next = (next + 1) % blocks.len();
            });
        });
        controller.collect_garbage();
    }
    group.finish();
}

criterion_group!(benches, engine);
criterion_main!(benches);
