//! `prisme latency`: round trip from an output to an input joined by a cable.

use anyhow::{bail, Context, Result};
use prisme_audio_io::latency::{round_trip_frames, LatencyProbe};
use prisme_audio_io::StreamSettings;
use std::time::{Duration, Instant};

/// Time between clicks: longer than any plausible round trip.
const CLICK_PERIOD_S: f64 = 0.25;
/// A click coming back is louder than this.
const THRESHOLD: f32 = 0.05;

/// `output_channel` and `input_channel` count from 1, as printed on the interface.
pub fn run(
    settings: &StreamSettings,
    output_channel: usize,
    input_channel: usize,
    seconds: u64,
) -> Result<()> {
    let mut reader = None;
    let mut period = 0;
    let stream = prisme_audio_io::start(settings, |format| {
        period = (format.sample_rate as f64 * CLICK_PERIOD_S) as usize;
        let capacity = format.sample_rate as usize * (seconds as usize + 1);
        let (probe, r) = LatencyProbe::new(
            output_channel.saturating_sub(1),
            input_channel.saturating_sub(1),
            period,
            capacity,
        );
        reader = Some(r);
        probe
    })?;
    let mut reader = reader.context("the probe was not created")?;
    let rate = stream.format.sample_rate as f64;
    println!(
        "clicks on output {output_channel}, listening on input {input_channel} ({} Hz, {} in / {} out)",
        stream.format.sample_rate, stream.format.input_channels, stream.format.output_channels
    );

    let mut recorded = Vec::new();
    let end = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
        recorded.extend(std::iter::from_fn(|| reader.pop().ok()));
    }
    drop(stream);

    let Some(frames) = round_trip_frames(&recorded, period, THRESHOLD) else {
        bail!("no click came back: check the cable from output {output_channel} to input {input_channel}");
    };
    println!("round trip: {frames} frames = {:.2} ms", frames as f64 / rate * 1000.0);
    Ok(())
}
