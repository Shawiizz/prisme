//! Cost of analysing a track once decoded (tempo, beat grid and key), and of its
//! waveform. Budget: the library analysed at 20 tracks per second or more on 8 cores,
//! decoding included.

use criterion::{criterion_group, criterion_main, Criterion};
use prisme_analysis::dsp::analysis_signal;
use prisme_analysis::key::estimate_key;
use prisme_analysis::onset::onset_envelopes;
use prisme_analysis::tempo::estimate_tempo;
use prisme_analysis::{analyze, compute_waveform, AnalysisOptions, DEFAULT_MIN_BPM};
use prisme_media::TrackAudio;
use std::f64::consts::TAU;

const RATE: u32 = 44_100;

/// Three minutes of kicks at 124 BPM over an A minor chord.
fn track() -> TrackAudio {
    let frames = 180 * RATE as usize;
    let beat = 60.0 / 124.0;
    let samples = (0..frames)
        .flat_map(|i| {
            let t = i as f64 / RATE as f64;
            let tt = t % beat;
            let kick = (-tt * 12.0).exp() * (TAU * 55.0 * tt).sin();
            let chord: f64 = [220.0, 261.63, 329.63].iter().map(|f| (TAU * f * t).sin()).sum();
            let v = (0.6 * kick + 0.1 * chord) as f32;
            [v, v]
        })
        .collect();
    TrackAudio::from_interleaved(RATE, samples)
}

fn analysis(c: &mut Criterion) {
    let track = track();
    let mut group = c.benchmark_group("track analysis");
    group.sample_size(20);
    group.bench_function("tempo and key, 3 min", |b| b.iter(|| analyze(&track, &AnalysisOptions::default())));
    let signal = analysis_signal(&track);
    let envelopes = onset_envelopes(&signal);
    group.bench_function("  mono 11 kHz", |b| b.iter(|| analysis_signal(&track)));
    group.bench_function("  onsets", |b| b.iter(|| onset_envelopes(&signal)));
    group.bench_function("  tempo", |b| b.iter(|| estimate_tempo(&envelopes, DEFAULT_MIN_BPM)));
    group.bench_function("  key", |b| b.iter(|| estimate_key(&signal)));
    group.bench_function("waveform, 3 min", |b| b.iter(|| compute_waveform(&track, 150)));
    group.finish();
}

criterion_group!(benches, analysis);
criterion_main!(benches);
