//! `prisme latency`: round trip from an output to an input joined by a cable.

use anyhow::Result;
use prisme_audio_io::latency::measure;
use prisme_audio_io::StreamSettings;
use std::time::Duration;

/// `output_channel` and `input_channel` count from 1, as printed on the interface.
pub fn run(
    settings: &StreamSettings,
    output_channel: usize,
    input_channel: usize,
    seconds: u64,
) -> Result<()> {
    println!("clicks on output {output_channel}, listening on input {input_channel}");
    let measured = measure(
        settings,
        output_channel.saturating_sub(1),
        input_channel.saturating_sub(1),
        Duration::from_secs(seconds),
    )?;
    let f = measured.format;
    println!(
        "round trip: {:.2} ms ({} Hz, {} in / {} out)",
        measured.round_trip.as_secs_f64() * 1000.0,
        f.sample_rate,
        f.input_channels,
        f.output_channels
    );
    Ok(())
}
