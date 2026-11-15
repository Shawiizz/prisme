//! Round-trip latency measurement: clicks are played on an output, a cable brings them
//! back to an input, and the delay is how late they arrive. It includes everything the
//! turntables go through: input, processing, output.

use crate::{AudioCallback, AudioError, StreamFormat, StreamSettings};
use std::time::{Duration, Instant};

/// Time between clicks: longer than any plausible round trip.
const CLICK_PERIOD_S: f64 = 0.25;
/// A click coming back is louder than this.
const THRESHOLD: f32 = 0.05;

#[derive(Debug, thiserror::Error)]
pub enum LatencyError {
    #[error(transparent)]
    Audio(#[from] AudioError),
    #[error("no click came back from output {output} to input {input}: is the cable there?")]
    NoClick { output: usize, input: usize },
}

/// A round trip measured, and the format it was measured in.
#[derive(Debug, Clone, Copy)]
pub struct Measured {
    pub round_trip: Duration,
    pub format: StreamFormat,
}

/// Plays clicks on `output_channel` for `length` and listens for them on `input_channel` (both
/// from 0), the devices of `settings` opened for this alone.
pub fn measure(
    settings: &StreamSettings,
    output_channel: usize,
    input_channel: usize,
    length: Duration,
) -> Result<Measured, LatencyError> {
    let mut reader = None;
    let mut period = 0;
    let stream = crate::start(settings, |format| {
        period = (f64::from(format.sample_rate) * CLICK_PERIOD_S) as usize;
        let capacity = (f64::from(format.sample_rate) * (length.as_secs_f64() + 1.0)) as usize;
        let (probe, r) = LatencyProbe::new(output_channel, input_channel, period, capacity);
        reader = Some(r);
        probe
    })?;
    let mut reader = reader.expect("the probe is made as the stream starts");
    let mut recorded = Vec::new();
    let end = Instant::now() + length;
    while Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
        recorded.extend(std::iter::from_fn(|| reader.pop().ok()));
    }
    let format = stream.format;
    drop(stream);
    let frames = round_trip_frames(&recorded, period, THRESHOLD)
        .ok_or(LatencyError::NoClick { output: output_channel + 1, input: input_channel + 1 })?;
    Ok(Measured {
        round_trip: Duration::from_secs_f64(frames as f64 / f64::from(format.sample_rate)),
        format,
    })
}

/// Plays a click every `period` frames on one output channel and records one input
/// channel into a lock-free ring.
pub struct LatencyProbe {
    output_channel: usize,
    input_channel: usize,
    period: usize,
    frame: usize,
    recorded: rtrb::Producer<f32>,
}

impl LatencyProbe {
    /// `capacity`: input frames that can wait for [`rtrb::Consumer`] reads.
    pub fn new(
        output_channel: usize,
        input_channel: usize,
        period: usize,
        capacity: usize,
    ) -> (Self, rtrb::Consumer<f32>) {
        let (recorded, reader) = rtrb::RingBuffer::new(capacity);
        (Self { output_channel, input_channel, period, frame: 0, recorded }, reader)
    }
}

impl AudioCallback for LatencyProbe {
    fn process(&mut self, input: &[f32], input_channels: usize, output: &mut [f32], output_channels: usize) {
        output.fill(0.0);
        let frames = output.len() / output_channels.max(1);
        for f in 0..frames {
            if (self.frame + f).is_multiple_of(self.period) && self.output_channel < output_channels {
                output[f * output_channels + self.output_channel] = 1.0;
            }
            let sample = if self.input_channel < input_channels {
                input.get(f * input_channels + self.input_channel).copied().unwrap_or(0.0)
            } else {
                0.0
            };
            let _ = self.recorded.push(sample);
        }
        self.frame += frames;
    }
}

/// How far above the noise floor (median level) a returning click must be.
const ABOVE_NOISE: f32 = 10.0;

/// Median delay (frames) between the clicks, sent at multiples of `period`, and their
/// return in `recorded`: the first sample louder than `min_threshold`, and well above the
/// noise floor, after each click. `None` if no click came back.
pub fn round_trip_frames(recorded: &[f32], period: usize, min_threshold: f32) -> Option<usize> {
    let threshold = min_threshold.max(noise_floor(recorded) * ABOVE_NOISE);
    let mut delays: Vec<usize> = recorded
        .chunks(period)
        .filter_map(|window| window.iter().position(|s| s.abs() > threshold))
        .collect();
    if delays.is_empty() {
        return None;
    }
    delays.sort_unstable();
    Some(delays[delays.len() / 2])
}

/// Median absolute level: the clicks are too rare to move it.
fn noise_floor(recorded: &[f32]) -> f32 {
    let mut levels: Vec<f32> = recorded.iter().map(|s| s.abs()).collect();
    if levels.is_empty() {
        return 0.0;
    }
    let middle = levels.len() / 2;
    *levels.select_nth_unstable_by(middle, f32::total_cmp).1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_delay_of_the_returning_clicks() {
        let period = 1000;
        let mut recorded = vec![0.0f32; period * 6];
        for (i, s) in recorded.iter_mut().enumerate() {
            *s = ((i * 7919 % 13) as f32 - 6.0) * 0.001; // background noise
        }
        for click in 0..6 {
            // the cable attenuates the click and smears it over a few samples
            let at = click * period + 237 + click % 2;
            recorded[at] = 0.4;
            recorded[at + 1] = -0.2;
        }
        assert_eq!(round_trip_frames(&recorded, period, 0.1), Some(238));
    }

    #[test]
    fn noise_louder_than_the_minimum_threshold_is_not_taken_for_a_click() {
        let period = 1000;
        let mut recorded: Vec<f32> = (0..period * 6).map(|i| ((i * 7919 % 13) as f32 - 6.0) * 0.02).collect();
        for click in 0..6 {
            recorded[click * period + 400] = 0.9;
        }
        assert_eq!(round_trip_frames(&recorded, period, 0.05), Some(400));
    }

    #[test]
    fn nothing_comes_back_without_a_cable() {
        assert_eq!(round_trip_frames(&[0.001; 5000], 1000, 0.1), None);
    }

    #[test]
    fn the_probe_clicks_on_its_channel_and_records_its_input() {
        let (mut probe, mut reader) = LatencyProbe::new(1, 0, 4, 64);
        let input = [0.5, 9.0, 0.25, 9.0, 0.0, 9.0, 0.0, 9.0, 0.75, 9.0, 0.0, 9.0];
        let mut output = [0.0f32; 12];
        probe.process(&input, 2, &mut output, 2);
        assert_eq!(output, [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        let recorded: Vec<f32> = std::iter::from_fn(|| reader.pop().ok()).collect();
        assert_eq!(recorded, [0.5, 0.25, 0.0, 0.0, 0.75, 0.0]);
    }
}
