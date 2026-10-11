//! Runs the timecode decoder over a recording of a turntable (the record playing, as a
//! WAV) and prints what it reads, then how steady it was: how soon the position was
//! found, how often it was known while the record played, how much the speed wavered
//! and how often the position jumped.

use anyhow::Context;
use prisme_timecode::{DecoderConfig, SignalState, TimecodeDecoder, TimecodeFormat};
use std::path::Path;

const BLOCK_FRAMES: usize = 64;
/// A position further than this from where the speed says it should be is a jump.
const JUMP_MS: f64 = 20.0;
/// Faster than this the record plays: the summary looks at those readings only.
const PLAYING_SPEED: f64 = 0.5;

struct Reading {
    time_s: f64,
    speed: f64,
    position_ms: Option<f64>,
}

pub fn run(
    file: &Path,
    format: &'static TimecodeFormat,
    swap_channels: bool,
    every_ms: f64,
) -> anyhow::Result<()> {
    let audio = prisme_media::decode_file(file).with_context(|| format!("cannot read {}", file.display()))?;
    let rate = f64::from(audio.sample_rate());
    let mut decoder =
        TimecodeDecoder::new(DecoderConfig { format, sample_rate: audio.sample_rate(), swap_channels });
    let blocks_per_line = ((every_ms / 1000.0 * rate) as usize / BLOCK_FRAMES).max(1);

    println!("{:>7}  {:>6}  {:>6}  {:>7}  {:>10}", "time s", "signal", "level", "speed", "record s");
    let mut readings = Vec::new();
    let mut block = 0;
    audio.for_each_block(BLOCK_FRAMES, |samples| {
        decoder.process(samples);
        block += 1;
        let s = decoder.status();
        let time_s = (block * BLOCK_FRAMES) as f64 / rate;
        readings.push(Reading { time_s, speed: s.speed, position_ms: s.position_ms });
        if block % blocks_per_line == 0 {
            let signal = match s.signal {
                SignalState::None => "none",
                SignalState::Bounce => "bounce",
                SignalState::Ok => "ok",
            };
            let position = s.position_ms.map_or("-".into(), |p| format!("{:.3}", p / 1000.0));
            println!("{time_s:>7.2}  {signal:>6}  {:>6.3}  {:>+7.4}  {position:>10}", s.level, s.speed);
        }
    });
    summarize(&readings);
    Ok(())
}

fn summarize(readings: &[Reading]) {
    let Some(first) = readings.iter().position(|r| r.position_ms.is_some()) else {
        println!("\nthe position was never found");
        return;
    };
    let locked = &readings[first..];
    let playing: Vec<&Reading> = locked.iter().filter(|r| r.speed.abs() > PLAYING_SPEED).collect();
    let n = playing.len().max(1) as f64;
    let known = playing.iter().filter(|r| r.position_ms.is_some()).count();
    let mean = playing.iter().map(|r| r.speed).sum::<f64>() / n;
    let deviation = (playing.iter().map(|r| (r.speed - mean).powi(2)).sum::<f64>() / n).sqrt();

    let mut jumps = 0;
    let mut worst_gap_ms: f64 = 0.0;
    for pair in locked.windows(2) {
        let (Some(a), Some(b)) = (pair[0].position_ms, pair[1].position_ms) else { continue };
        let expected = (pair[1].time_s - pair[0].time_s) * 1000.0 * pair[1].speed;
        let gap = (b - a - expected).abs();
        worst_gap_ms = worst_gap_ms.max(gap);
        if gap > JUMP_MS {
            jumps += 1;
        }
    }

    println!("\nposition found after {:.0} ms", readings[first].time_s * 1000.0);
    println!("position known {:.1} % of the time the record played since", known as f64 / n * 100.0);
    println!(
        "speed while playing {mean:+.4} on average, wavering by {:.2} % (standard deviation)",
        deviation / mean.abs() * 100.0
    );
    println!("position jumps over {JUMP_MS} ms: {jumps}; largest gap between readings {worst_gap_ms:.1} ms");
}
