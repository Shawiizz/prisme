//! Worst audio callback over a minute of playback: two decks, one with the key lock, one
//! on a turntable, through the software mixer. The average hides what matters in real
//! time: the slowest callback must fit in the buffer's duration.
//!
//! `cargo run --release -p prisme-engine --example callback_peaks [frames] [key-locked decks]`

use prisme_engine::{create, Command, EngineConfig, VinylMode};
use prisme_media::TrackAudio;
use prisme_timecode::testing::SignalGenerator;
use prisme_timecode::SERATO_CV02_A;
use std::sync::Arc;
use std::time::Instant;

const RATE: u32 = 48_000;

fn main() {
    let arg =
        |i: usize, default: usize| std::env::args().nth(i).and_then(|a| a.parse().ok()).unwrap_or(default);
    let (frames, key_locked) = (arg(1, 64), arg(2, 1));
    let track = |seconds: usize| {
        let samples = (0..seconds * RATE as usize * 2).map(|i| ((i as f32) * 0.001).sin() * 0.5).collect();
        Arc::new(TrackAudio::from_interleaved(RATE, samples))
    };
    let (mut engine, mut controller) = create(EngineConfig::new(RATE, 2, &SERATO_CV02_A));
    for command in [
        Command::LoadTrack { deck: 0, track: track(90) },
        Command::LoadTrack { deck: 1, track: track(90) },
        Command::SetPlaying { deck: 0, playing: true },
        Command::SetPitch { deck: 0, ratio: 1.04 },
        Command::SetKeyLock { deck: 0, enabled: key_locked >= 1 },
        Command::SetVinylEnabled { deck: 1, enabled: true },
        Command::SetVinylMode { deck: 1, mode: VinylMode::Relative },
        Command::SetKeyLock { deck: 1, enabled: key_locked >= 2 },
    ] {
        assert!(controller.send(command).is_ok());
    }
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
    let mut output = vec![0.0; frames * 4];
    let callbacks = 60 * RATE as usize / frames;
    let mut times = Vec::with_capacity(callbacks);
    for i in 0..callbacks {
        let t = Instant::now();
        engine.process(&blocks[i % blocks.len()], 4, &mut output, 4);
        times.push(t.elapsed().as_secs_f64() * 1e6);
        if i % 64 == 0 {
            controller.collect_garbage();
        }
    }
    let budget = frames as f64 / RATE as f64 * 1e6;
    let average = times.iter().sum::<f64>() / times.len() as f64;
    times.sort_by(f64::total_cmp);
    let at = |p: f64| times[((times.len() - 1) as f64 * p) as usize];
    let (p99, p999, worst) = (at(0.99), at(0.999), at(1.0));
    // the worst callbacks include the times the OS preempted this (non real-time) thread
    println!(
        "{frames} frames ({budget:.0} us), {key_locked} key-locked: average {average:.1} us, 99 % {p99:.0} us, 99.9 % {p999:.0} us ({:.0} % of the buffer), worst {worst:.0} us",
        p999 / budget * 100.0
    );
}
