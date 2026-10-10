//! Running the audio: the output stream drives the callback; an optional input stream
//! feeds it through a lock-free ring buffer. With ASIO both streams come from the same
//! driver period, so the ring never holds more than a period.

use crate::devices::{device_name, find, Backend, DeviceKind};
use crate::formats::{input_stream, output_stream, MAX_PERIOD};
use crate::{AudioCallback, AudioError};
use cpal::traits::{DeviceTrait, StreamTrait};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone, Default)]
pub struct StreamSettings {
    pub backend: Backend,
    /// Part of the output device name, or `None` for the default device.
    pub output_device: Option<String>,
    /// Part of the input device name. `None` = no input (no turntables).
    pub input_device: Option<String>,
    /// Requested period in frames (`None` = device default). Not every driver honours it.
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
    /// The device went away (unplugged): the stream must be restarted.
    pub device_lost: AtomicBool,
    /// The callback panicked: it is no longer called, the output is silent.
    pub callback_failed: AtomicBool,
}

/// What the opened devices actually run at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamFormat {
    pub sample_rate: u32,
    pub input_channels: usize,
    pub output_channels: usize,
    /// Period requested from the driver (`None`: the driver's default).
    pub buffer_frames: Option<u32>,
}

/// A running audio setup. Dropping it stops the audio.
pub struct RunningStream {
    _output: cpal::Stream,
    _input: Option<cpal::Stream>,
    pub format: StreamFormat,
    pub stats: Arc<StreamStats>,
}

/// Opens the devices, builds the callback for their format and starts calling it.
pub fn start<C: AudioCallback>(
    settings: &StreamSettings,
    make_callback: impl FnOnce(StreamFormat) -> C,
) -> Result<RunningStream, AudioError> {
    let out_device = find(settings.backend, DeviceKind::Output, settings.output_device.as_deref())?;
    let out_default = out_device.default_output_config()?;
    let sample_rate = out_default.sample_rate();
    let output_channels = out_default.channels() as usize;
    let mut out_config = out_default.config();
    let buffer_frames =
        settings.buffer_frames.map(|frames| supported_buffer(frames, out_default.buffer_size()));
    if let Some(frames) = buffer_frames {
        out_config.buffer_size = cpal::BufferSize::Fixed(frames);
    }

    let stats = Arc::new(StreamStats::default());
    let (input_stream, ring, input_channels) = match settings.input_device.as_deref() {
        Some(name) => {
            let input = open_input(settings.backend, name, &out_device, sample_rate, &stats)?;
            (Some(input.stream), Some(input.ring), input.channels)
        }
        None => (None, None, 0),
    };

    let format = StreamFormat { sample_rate, input_channels, output_channels, buffer_frames };
    let mut renderer = Renderer {
        callback: make_callback(format),
        ring,
        input: vec![0.0; MAX_PERIOD * input_channels.max(1)],
        input_channels,
        output_channels,
        sample_rate,
        stats: stats.clone(),
        failed: false,
    };
    let output = output_stream(
        &out_device,
        out_config,
        out_default.sample_format(),
        move |data| renderer.render(data),
        error_handler(stats.clone()),
    )?;

    if let Some(i) = input_stream.as_ref() {
        i.play()?;
    }
    output.play()?;
    Ok(RunningStream { _output: output, _input: input_stream, format, stats })
}

struct Input {
    stream: cpal::Stream,
    ring: rtrb::Consumer<f32>,
    channels: usize,
}

fn open_input(
    backend: Backend,
    name: &str,
    out_device: &cpal::Device,
    sample_rate: u32,
    stats: &Arc<StreamStats>,
) -> Result<Input, AudioError> {
    // an ASIO driver is one device for both directions: open it once
    let same_driver = backend == Backend::Asio
        && device_name(out_device).is_some_and(|n| n.to_lowercase().contains(&name.to_lowercase()));
    let device = if same_driver { out_device.clone() } else { find(backend, DeviceKind::Input, Some(name))? };
    let default = device.default_input_config()?;
    if default.sample_rate() != sample_rate {
        return Err(AudioError::SampleRateMismatch { input: default.sample_rate(), output: sample_rate });
    }
    let channels = default.channels() as usize;
    let (mut tx, ring) = rtrb::RingBuffer::<f32>::new(sample_rate as usize * channels / 2);
    let st = stats.clone();
    let push = move |data: &[f32]| {
        let n = data.len().min(tx.slots());
        if let Ok(chunk) = tx.write_chunk_uninit(n) {
            chunk.fill_from_iter(data[..n].iter().copied());
        }
        if n < data.len() {
            st.input_dropped.fetch_add(((data.len() - n) / channels) as u64, Ordering::Relaxed);
        }
    };
    let stream =
        input_stream(&device, default.config(), default.sample_format(), push, error_handler(stats.clone()))?;
    Ok(Input { stream, ring, channels })
}

/// What runs in the output callback: gathers the input that arrived, calls the
/// callback, keeps the statistics.
struct Renderer<C> {
    callback: C,
    ring: Option<rtrb::Consumer<f32>>,
    input: Vec<f32>,
    input_channels: usize,
    output_channels: usize,
    sample_rate: u32,
    stats: Arc<StreamStats>,
    /// The callback panicked: its state cannot be trusted any more.
    failed: bool,
}

impl<C: AudioCallback> Renderer<C> {
    fn render(&mut self, output: &mut [f32]) {
        let start = Instant::now();
        let frames = output.len() / self.output_channels;
        // a panic must not unwind into the driver (that aborts the whole program)
        if !self.failed && catch_unwind(AssertUnwindSafe(|| self.process(output, frames))).is_err() {
            self.failed = true;
            self.stats.callback_failed.store(true, Ordering::Relaxed);
        }
        if self.failed {
            output.fill(0.0);
        }
        let ns = start.elapsed().as_nanos() as u64;
        let budget = (frames as f64 / self.sample_rate as f64 * 1e9) as u64;
        self.stats.callbacks.fetch_add(1, Ordering::Relaxed);
        self.stats.frames.fetch_add(frames as u64, Ordering::Relaxed);
        self.stats.max_callback_ns.fetch_max(ns, Ordering::Relaxed);
        if ns > budget {
            self.stats.over_budget.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl<C: AudioCallback> Renderer<C> {
    fn process(&mut self, output: &mut [f32], frames: usize) {
        match self.ring.as_mut() {
            Some(ring) => {
                let want = (frames * self.input_channels).min(self.input.len());
                read_input(ring, &mut self.input[..want], self.input_channels, &self.stats);
                self.callback.process(&self.input[..want], self.input_channels, output, self.output_channels);
            }
            None => self.callback.process(&[], 0, output, self.output_channels),
        }
    }
}

/// Fills `into` from the ring, padding with silence; more than two periods waiting are
/// dropped, oldest first, to keep the input latency low.
fn read_input(ring: &mut rtrb::Consumer<f32>, into: &mut [f32], channels: usize, stats: &StreamStats) {
    let want = into.len();
    let excess = ring.slots().saturating_sub(want * 2);
    let excess = excess - excess % channels;
    if excess > 0 {
        if let Ok(chunk) = ring.read_chunk(excess) {
            chunk.commit_all();
        }
        stats.input_dropped.fetch_add((excess / channels) as u64, Ordering::Relaxed);
    }
    let available = ring.slots().min(want);
    let available = available - available % channels;
    let mut got = 0;
    if let Ok(chunk) = ring.read_chunk(available) {
        let (a, b) = chunk.as_slices();
        into[..a.len()].copy_from_slice(a);
        into[a.len()..a.len() + b.len()].copy_from_slice(b);
        chunk.commit_all();
        got = available;
    }
    if got < want {
        into[got..].fill(0.0);
        stats.input_underruns.fetch_add(((want - got) / channels) as u64, Ordering::Relaxed);
    }
}

/// The requested period, brought into the range the driver accepts.
fn supported_buffer(frames: u32, supported: &cpal::SupportedBufferSize) -> u32 {
    match *supported {
        cpal::SupportedBufferSize::Range { min, max } => frames.clamp(min, max),
        cpal::SupportedBufferSize::Unknown => frames,
    }
}

fn error_handler(stats: Arc<StreamStats>) -> impl FnMut(cpal::Error) + Send + 'static {
    move |e| {
        if e.kind() == cpal::ErrorKind::DeviceNotAvailable {
            stats.device_lost.store(true, Ordering::Relaxed);
        }
        eprintln!("audio stream error: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plays a tone, and panics on its `fail_at`th call.
    struct Faulty {
        calls: usize,
        fail_at: usize,
    }

    impl AudioCallback for Faulty {
        fn process(&mut self, _: &[f32], _: usize, output: &mut [f32], _: usize) {
            self.calls += 1;
            assert!(self.calls != self.fail_at, "engine bug");
            output.fill(0.5);
        }
    }

    fn renderer(fail_at: usize) -> Renderer<Faulty> {
        Renderer {
            callback: Faulty { calls: 0, fail_at },
            ring: None,
            input: Vec::new(),
            input_channels: 0,
            output_channels: 2,
            sample_rate: 48_000,
            stats: Arc::new(StreamStats::default()),
            failed: false,
        }
    }

    #[test]
    fn a_panicking_callback_is_silenced_and_never_called_again() {
        let mut r = renderer(2);
        let mut output = [0.0f32; 128];
        r.render(&mut output);
        assert!(output.iter().all(|&s| s == 0.5));
        assert!(!r.stats.callback_failed.load(Ordering::Relaxed));

        r.render(&mut output);
        r.render(&mut output);
        assert!(output.iter().all(|&s| s == 0.0));
        assert!(r.stats.callback_failed.load(Ordering::Relaxed));
        assert_eq!(r.callback.calls, 2);
        assert_eq!(r.stats.callbacks.load(Ordering::Relaxed), 3);
    }
}
