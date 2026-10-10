//! Running the audio: an output stream that drives the callback, and an optional input
//! stream whose samples reach the callback through a lock-free ring buffer.

use crate::devices::{find, DeviceKind};
use crate::{AudioCallback, AudioError};
use cpal::traits::{DeviceTrait, StreamTrait};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Largest period we prepare buffers for (frames).
const MAX_PERIOD: usize = 16_384;

#[derive(Debug, Clone, Default)]
pub struct StreamSettings {
    /// Part of the output device name, or `None` for the default device.
    pub output_device: Option<String>,
    /// Part of the input device name. `None` = no input (no turntables).
    pub input_device: Option<String>,
    /// Requested period in frames (`None` = device default). Not every backend honours it.
    pub buffer_frames: Option<u32>,
}

/// Counters updated by the audio thread.
#[derive(Debug, Default)]
pub struct StreamStats {
    pub callbacks: AtomicU64,
    pub frames: AtomicU64,
    /// Longest time spent in the callback (ns).
    pub max_callback_ns: AtomicU64,
    /// Callbacks that took longer than their period.
    pub over_budget: AtomicU64,
    /// Input frames missing when the output needed them (padded with silence).
    pub input_underruns: AtomicU64,
    /// Input frames dropped because the ring buffer was full, or to limit latency.
    pub input_dropped: AtomicU64,
}

/// A running audio setup. Dropping it stops the audio.
pub struct RunningStream {
    _output: cpal::Stream,
    _input: Option<cpal::Stream>,
    pub sample_rate: u32,
    pub output_channels: usize,
    pub input_channels: usize,
    pub stats: Arc<StreamStats>,
}

/// Opens the devices and starts calling `callback`.
pub fn start(
    settings: &StreamSettings,
    mut callback: impl AudioCallback,
) -> Result<RunningStream, AudioError> {
    let out_device = find(DeviceKind::Output, settings.output_device.as_deref())?;
    let out_default = out_device.default_output_config()?;
    if out_default.sample_format() != cpal::SampleFormat::F32 {
        return Err(AudioError::UnsupportedFormat(out_default.sample_format()));
    }
    let sample_rate = out_default.sample_rate().0;
    let output_channels = out_default.channels() as usize;
    let mut out_config: cpal::StreamConfig = out_default.config();
    if let Some(frames) = settings.buffer_frames {
        out_config.buffer_size = cpal::BufferSize::Fixed(frames);
    }

    let stats = Arc::new(StreamStats::default());

    // Input: its own stream feeding a ring buffer read by the output callback.
    let (input, mut ring_rx, input_channels) = match settings.input_device.as_deref() {
        None => (None, None, 0usize),
        Some(name) => {
            let in_device = find(DeviceKind::Input, Some(name))?;
            let in_default = in_device.default_input_config()?;
            if in_default.sample_rate().0 != sample_rate {
                return Err(AudioError::SampleRateMismatch {
                    input: in_default.sample_rate().0,
                    output: sample_rate,
                });
            }
            let channels = in_default.channels() as usize;
            let (mut tx, rx) = rtrb::RingBuffer::<f32>::new(sample_rate as usize * channels / 2);
            let in_config: cpal::StreamConfig = in_default.config();
            let st = stats.clone();
            let mut push = move |data: &[f32]| {
                let n = data.len().min(tx.slots());
                if let Ok(chunk) = tx.write_chunk_uninit(n) {
                    chunk.fill_from_iter(data[..n].iter().copied());
                }
                if n < data.len() {
                    st.input_dropped.fetch_add(((data.len() - n) / channels) as u64, Ordering::Relaxed);
                }
            };
            let stream = match in_default.sample_format() {
                cpal::SampleFormat::F32 => {
                    in_device.build_input_stream(&in_config, move |d: &[f32], _| push(d), log_error, None)?
                }
                cpal::SampleFormat::I16 => {
                    let mut scratch = vec![0.0f32; MAX_PERIOD * channels];
                    in_device.build_input_stream(
                        &in_config,
                        move |d: &[i16], _| {
                            let n = d.len().min(scratch.len());
                            for (o, &s) in scratch[..n].iter_mut().zip(d) {
                                *o = s as f32 / 32768.0;
                            }
                            push(&scratch[..n]);
                        },
                        log_error,
                        None,
                    )?
                }
                other => return Err(AudioError::UnsupportedFormat(other)),
            };
            (Some(stream), Some(rx), channels)
        }
    };

    let mut in_buf = vec![0.0f32; MAX_PERIOD * input_channels.max(1)];
    let st = stats.clone();
    let output = out_device.build_output_stream(
        &out_config,
        move |data: &mut [f32], _| {
            let start = Instant::now();
            let frames = data.len() / output_channels;
            let mut got = 0;
            if let Some(rx) = ring_rx.as_mut() {
                let want = (frames * input_channels).min(in_buf.len());
                // keep the input latency low: if more than two periods are waiting, drop the oldest
                let excess = rx.slots().saturating_sub(want * 2);
                let excess = excess - excess % input_channels;
                if excess > 0 {
                    if let Ok(chunk) = rx.read_chunk(excess) {
                        chunk.commit_all();
                    }
                    st.input_dropped.fetch_add((excess / input_channels) as u64, Ordering::Relaxed);
                }
                let available = rx.slots().min(want);
                let available = available - available % input_channels;
                if let Ok(chunk) = rx.read_chunk(available) {
                    let (a, b) = chunk.as_slices();
                    in_buf[..a.len()].copy_from_slice(a);
                    in_buf[a.len()..a.len() + b.len()].copy_from_slice(b);
                    chunk.commit_all();
                    got = available;
                }
                if got < want {
                    in_buf[got..want].fill(0.0);
                    st.input_underruns.fetch_add(((want - got) / input_channels) as u64, Ordering::Relaxed);
                }
                callback.process(&in_buf[..want], input_channels, data, output_channels);
            } else {
                callback.process(&[], 0, data, output_channels);
            }
            let ns = start.elapsed().as_nanos() as u64;
            let budget = (frames as f64 / sample_rate as f64 * 1e9) as u64;
            st.callbacks.fetch_add(1, Ordering::Relaxed);
            st.frames.fetch_add(frames as u64, Ordering::Relaxed);
            st.max_callback_ns.fetch_max(ns, Ordering::Relaxed);
            if ns > budget {
                st.over_budget.fetch_add(1, Ordering::Relaxed);
            }
        },
        log_error,
        None,
    )?;

    if let Some(i) = input.as_ref() {
        i.play()?;
    }
    output.play()?;
    Ok(RunningStream { _output: output, _input: input, sample_rate, output_channels, input_channels, stats })
}

fn log_error(e: cpal::StreamError) {
    eprintln!("audio stream error: {e}");
}
