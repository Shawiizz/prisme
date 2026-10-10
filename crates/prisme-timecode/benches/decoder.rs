//! Cost of decoding one turntable's timecode, in the 128-frame blocks of a low-latency
//! callback. Budget: under 1 % of a core at 96 kHz, i.e. one second of signal decoded in
//! under 10 ms.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use prisme_timecode::testing::SignalGenerator;
use prisme_timecode::{DecoderConfig, TimecodeDecoder, SERATO_CV02_A};

const BLOCK_FRAMES: usize = 128;

fn decoder(c: &mut Criterion) {
    let mut group = c.benchmark_group("timecode decoder");
    for rate in [48_000u32, 96_000] {
        let mut signal = Vec::new();
        // a record turning at nominal speed, with the slight wobble of a real platter
        SignalGenerator::new(&SERATO_CV02_A, rate, 60_000.0, 2_000.0).render(
            rate as usize,
            |i| 1.0 + 0.002 * (i as f64 / rate as f64 * std::f64::consts::TAU).sin(),
            &mut signal,
        );
        group.throughput(Throughput::Elements(rate as u64));
        group.bench_with_input(BenchmarkId::new("one second", rate), &signal, |b, signal| {
            let config = DecoderConfig { format: &SERATO_CV02_A, sample_rate: rate, swap_channels: false };
            let mut decoder = TimecodeDecoder::new(config);
            b.iter(|| {
                for block in signal.chunks(2 * BLOCK_FRAMES) {
                    decoder.process(block);
                }
            });
        });
    }
    group.finish();
}

criterion_group!(benches, decoder);
criterion_main!(benches);
