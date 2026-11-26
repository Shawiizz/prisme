//! Running the audio: the output stream drives the callback; an optional input stream
//! feeds it through a lock-free ring buffer. With ASIO both streams come from the same
//! driver period, so the ring never holds more than a period.

use crate::devices::{configs, device_ref, drivers, find, Backend, DeviceKind, DeviceRef};
use crate::formats::{input_stream, output_stream, MAX_PERIOD};
use crate::perf::{PerformanceHint, Refused};
use crate::{AudioCallback, AudioError};
use cpal::traits::{DeviceTrait, StreamTrait};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::warn;

#[derive(Debug, Clone, Default)]
pub struct StreamSettings {
    pub backend: Backend,
    /// The output, or `None` for the default device.
    pub output_device: Option<DeviceRef>,
    /// The input. `None` = no input (no turntables).
    pub input_device: Option<DeviceRef>,
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
    /// The device went away or changed (unplugged, another default device, another format):
    /// the stream must be restarted.
    pub device_lost: AtomicBool,
    /// The callback panicked: it is no longer called, the output is silent.
    pub callback_failed: AtomicBool,
    /// The system has performance hints but gave the audio thread none (told when the stream
    /// stops, away from the audio thread).
    pub hints_refused: AtomicBool,
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
    /// Closed under the drivers' lock.
    streams: Option<Streams>,
    pub format: StreamFormat,
    pub stats: Arc<StreamStats>,
    /// The output opened, as its system knows it.
    pub output_device: DeviceRef,
}

/// What keeps the sound going.
pub(crate) enum Streams {
    /// The system's, playing as long as they are kept.
    Cpal { _output: cpal::Stream, _input: Option<cpal::Stream> },
    /// A USB card Prisme drives itself.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    Usb { _card: crate::usb::Running },
}

impl RunningStream {
    pub(crate) fn new(
        streams: Streams,
        format: StreamFormat,
        stats: Arc<StreamStats>,
        output_device: DeviceRef,
    ) -> Self {
        Self { streams: Some(streams), format, stats, output_device }
    }
}

/// Opens the devices, builds the callback for their format and starts calling it (again
/// for each configuration of the output tried, if the first is refused).
pub fn start<C: AudioCallback>(
    settings: &StreamSettings,
    mut make_callback: impl FnMut(StreamFormat) -> C,
) -> Result<RunningStream, AudioError> {
    // a USB card loads no driver: the lock is not held
    if settings.backend == Backend::Usb {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        return crate::usb::start(settings, make_callback);
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        return Err(AudioError::BackendUnavailable(Backend::Usb));
    }
    let _drivers = drivers();
    let out_device = find(settings.backend, DeviceKind::Output, settings.output_device.as_ref())?;
    let mut tried = configs(&out_device, DeviceKind::Output)?.into_iter().peekable();
    loop {
        let config = tried.next().expect("a device has a configuration");
        match start_with(settings, &out_device, config, &mut make_callback) {
            Err(e) if tried.peek().is_some() => {
                warn!("output refused with all its channels ({e}): trying fewer")
            }
            started => return started,
        }
    }
}

/// Whether the output of `settings` opens now, tried and closed at once, the audio running
/// elsewhere untouched: another application holding it, it does not. A USB card, and an ASIO
/// one while another ASIO driver plays (one at a time), are not tried.
pub fn output_opens(settings: &StreamSettings) -> bool {
    if settings.backend == Backend::Usb {
        return false;
    }
    let _drivers = drivers();
    let Ok(device) = find(settings.backend, DeviceKind::Output, settings.output_device.as_ref()) else {
        return false;
    };
    let Ok(config) = device.default_output_config() else { return false };
    device
        .build_output_stream_raw(
            config.config(),
            config.sample_format(),
            |_: &mut cpal::Data, _: &cpal::OutputCallbackInfo| {},
            |_| {},
            None,
        )
        .is_ok()
}

fn start_with<C: AudioCallback>(
    settings: &StreamSettings,
    out_device: &cpal::Device,
    out_default: cpal::SupportedStreamConfig,
    make_callback: &mut impl FnMut(StreamFormat) -> C,
) -> Result<RunningStream, AudioError> {
    let output_device = device_ref(out_device).ok_or(AudioError::NoOutputDevice)?;
    let mut sample_rate = out_default.sample_rate();
    let output_channels = out_default.channels() as usize;
    let mut out_config = out_default.config();
    // The system's shared output takes any rate (the system converts it): it follows the
    // input, whose rate the timecode needs as it is, when the system set them apart.
    if settings.backend == Backend::SYSTEM {
        if let Some(rate) = settings.input_device.as_ref().and_then(input_rate) {
            sample_rate = rate;
            out_config.sample_rate = rate;
        }
    }
    let buffer_frames =
        settings.buffer_frames.map(|frames| supported_buffer(frames, out_default.buffer_size()));
    if let Some(frames) = buffer_frames {
        out_config.buffer_size = cpal::BufferSize::Fixed(frames);
    }

    let stats = Arc::new(StreamStats::default());
    let (input_stream, ring, input_channels) = match settings.input_device.as_ref() {
        Some(wanted) => {
            let input = open_input(settings.backend, wanted, out_device, sample_rate, &stats)?;
            (Some(input.stream), Some(input.ring), input.channels)
        }
        None => (None, None, 0),
    };

    let format = StreamFormat { sample_rate, input_channels, output_channels, buffer_frames };
    let mut renderer = Renderer::new(make_callback(format), ring, format, stats.clone());
    let output = output_stream(
        out_device,
        out_config,
        out_default.sample_format(),
        move |data| renderer.render(data),
        error_handler(stats.clone()),
    )?;

    if let Some(i) = input_stream.as_ref() {
        i.play()?;
    }
    output.play()?;
    Ok(RunningStream::new(
        Streams::Cpal { _output: output, _input: input_stream },
        format,
        stats,
        output_device,
    ))
}

impl Drop for RunningStream {
    fn drop(&mut self) {
        let _drivers = drivers();
        self.streams = None;
        if self.stats.hints_refused.load(Ordering::Relaxed) {
            warn!("the system gave the audio thread no performance hints");
        }
    }
}

/// The rate the system runs input `wanted` at, if it is there.
fn input_rate(wanted: &DeviceRef) -> Option<u32> {
    let device = find(Backend::SYSTEM, DeviceKind::Input, Some(wanted)).ok()?;
    Some(device.default_input_config().ok()?.sample_rate())
}

struct Input {
    stream: cpal::Stream,
    ring: rtrb::Consumer<f32>,
    channels: usize,
}

fn open_input(
    backend: Backend,
    wanted: &DeviceRef,
    out_device: &cpal::Device,
    sample_rate: u32,
    stats: &Arc<StreamStats>,
) -> Result<Input, AudioError> {
    // an ASIO driver, as the JACK server, is one device for both directions: open it once
    let same_driver = match backend {
        Backend::Jack => true,
        Backend::Asio => device_ref(out_device).is_some_and(|out| out.id == wanted.id),
        _ => false,
    };
    let device =
        if same_driver { out_device.clone() } else { find(backend, DeviceKind::Input, Some(wanted))? };
    let mut tried = configs(&device, DeviceKind::Input)?.into_iter().peekable();
    loop {
        let config = tried.next().expect("a device has a configuration");
        match open_input_with(&device, config, sample_rate, stats) {
            Err(e) if tried.peek().is_some() => {
                warn!("input refused with all its channels ({e}): trying fewer")
            }
            opened => return opened,
        }
    }
}

fn open_input_with(
    device: &cpal::Device,
    default: cpal::SupportedStreamConfig,
    sample_rate: u32,
    stats: &Arc<StreamStats>,
) -> Result<Input, AudioError> {
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
        input_stream(device, default.config(), default.sample_format(), push, error_handler(stats.clone()))?;
    Ok(Input { stream, ring, channels })
}

/// What runs in the output callback: gathers the input that arrived, calls the
/// callback, keeps the statistics.
pub(crate) struct Renderer<C> {
    callback: C,
    ring: Option<InputRing>,
    input: Vec<f32>,
    input_channels: usize,
    output_channels: usize,
    sample_rate: u32,
    stats: Arc<StreamStats>,
    /// A callback longer than its period is late: not where the device holds several periods
    /// and tells itself when it ran out of them.
    times_itself: bool,
    /// The system told how long each period takes, from the first one (on the audio thread).
    hint: Option<PerformanceHint>,
    hinted: bool,
    /// The callback panicked: its state cannot be trusted any more.
    failed: bool,
}

impl<C: AudioCallback> Renderer<C> {
    /// Calls `callback` for the output of `format`, with what came into `ring` from its input.
    pub(crate) fn new(
        callback: C,
        ring: Option<rtrb::Consumer<f32>>,
        format: StreamFormat,
        stats: Arc<StreamStats>,
    ) -> Self {
        PerformanceHint::prepare();
        Self {
            callback,
            ring: ring.map(|ring| InputRing { ring, cushion: 0, slack: None, primed: true }),
            input: vec![0.0; MAX_PERIOD * format.input_channels.max(1)],
            input_channels: format.input_channels,
            output_channels: format.output_channels,
            sample_rate: format.sample_rate,
            stats,
            times_itself: true,
            hint: None,
            hinted: false,
            failed: false,
        }
    }

    /// Keeps `cushion` frames of the input ahead of what the callback reads, and lets up to
    /// `slack` more wait before any is dropped: an input coming in bursts of its own, on the
    /// clock of the output but apart from it, would otherwise run short at times or overflow,
    /// and the turntables' signal skip.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub(crate) fn input_cushion(mut self, cushion: usize, slack: usize) -> Self {
        if let Some(ring) = self.ring.as_mut() {
            ring.cushion = cushion * self.input_channels;
            ring.slack = Some(slack * self.input_channels);
            ring.primed = ring.cushion == 0;
        }
        self
    }

    /// Takes the input up again from its cushion (the output skipped a while: the input that
    /// came meanwhile is not to be played late).
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub(crate) fn resync_input(&mut self) {
        if let Some(ring) = self.ring.as_mut().filter(|r| r.cushion > 0) {
            ring.primed = false;
        }
    }

    /// Leaves telling a late period to the device ([`Renderer::times_itself`]).
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub(crate) fn late_told_by_the_device(mut self) -> Self {
        self.times_itself = false;
        self
    }

    pub(crate) fn render(&mut self, output: &mut [f32]) {
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
        if !std::mem::replace(&mut self.hinted, true) {
            match PerformanceHint::for_this_thread(Duration::from_nanos(budget)) {
                Ok(hint) => self.hint = hint,
                Err(Refused) => self.stats.hints_refused.store(true, Ordering::Relaxed),
            }
        }
        if let Some(hint) = self.hint.as_mut() {
            hint.report(Duration::from_nanos(ns));
        }
        self.stats.callbacks.fetch_add(1, Ordering::Relaxed);
        self.stats.frames.fetch_add(frames as u64, Ordering::Relaxed);
        self.stats.max_callback_ns.fetch_max(ns, Ordering::Relaxed);
        if self.times_itself && ns > budget {
            self.stats.over_budget.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl<C: AudioCallback> Renderer<C> {
    fn process(&mut self, output: &mut [f32], frames: usize) {
        match self.ring.as_mut() {
            Some(ring) => {
                let want = (frames * self.input_channels).min(self.input.len());
                ring.read(&mut self.input[..want], self.input_channels, &self.stats);
                self.callback.process(&self.input[..want], self.input_channels, output, self.output_channels);
            }
            None => self.callback.process(&[], 0, output, self.output_channels),
        }
    }
}

/// What came in from the input, waiting for the output.
pub(crate) struct InputRing {
    ring: rtrb::Consumer<f32>,
    /// Samples kept ahead of what is read, and more let wait before any is dropped (a period
    /// when `None`) ([`Renderer::input_cushion`]).
    cushion: usize,
    slack: Option<usize>,
    /// The cushion is there: the input is read. Until then, silence.
    primed: bool,
}

impl InputRing {
    /// Fills `into` from the ring, padding with silence; more than the cushion, a period and the
    /// slack waiting are dropped, oldest first, to keep the input latency low. Run short, the
    /// ring builds its cushion again, and starts from it.
    fn read(&mut self, into: &mut [f32], channels: usize, stats: &StreamStats) {
        let want = into.len();
        if !self.primed {
            into.fill(0.0);
            let waiting = self.ring.slots();
            if waiting < want + self.cushion {
                return;
            }
            // from the cushion on: what came before is older than the cushion allows, and kept
            // it would leave the input that much later, and a burst the less room
            let older = waiting - want - self.cushion;
            if let Ok(chunk) = self.ring.read_chunk(older - older % channels) {
                chunk.commit_all();
            }
            self.primed = true;
        }
        let excess = self.ring.slots().saturating_sub(want + self.cushion + self.slack.unwrap_or(want));
        let excess = excess - excess % channels;
        if excess > 0 {
            if let Ok(chunk) = self.ring.read_chunk(excess) {
                chunk.commit_all();
            }
            stats.input_dropped.fetch_add((excess / channels) as u64, Ordering::Relaxed);
        }
        let available = self.ring.slots().min(want);
        let available = available - available % channels;
        let mut got = 0;
        if let Ok(chunk) = self.ring.read_chunk(available) {
            let (a, b) = chunk.as_slices();
            into[..a.len()].copy_from_slice(a);
            into[a.len()..a.len() + b.len()].copy_from_slice(b);
            chunk.commit_all();
            got = available;
        }
        if got < want {
            into[got..].fill(0.0);
            stats.input_underruns.fetch_add(((want - got) / channels) as u64, Ordering::Relaxed);
            self.primed = self.cushion == 0;
        }
    }
}

/// The requested period, brought into the range the driver accepts.
fn supported_buffer(frames: u32, supported: &cpal::SupportedBufferSize) -> u32 {
    match *supported {
        cpal::SupportedBufferSize::Range { min, max } => frames.clamp(min, max),
        cpal::SupportedBufferSize::Unknown => frames,
    }
}

/// A driver in trouble tells the same error thousands of times a second: the log takes one
/// line a second, with how many it stands for.
const ERRORS_LOGGED_EVERY: Duration = Duration::from_secs(1);

fn error_handler(stats: Arc<StreamStats>) -> impl FnMut(cpal::Error) + Send + 'static {
    let mut logged: Option<Instant> = None;
    let mut unlogged = 0u32;
    move |e| {
        if matches!(e.kind(), cpal::ErrorKind::DeviceNotAvailable | cpal::ErrorKind::StreamInvalidated) {
            stats.device_lost.store(true, Ordering::Relaxed);
        }
        if logged.is_some_and(|at| at.elapsed() < ERRORS_LOGGED_EVERY) {
            unlogged += 1;
            return;
        }
        match std::mem::take(&mut unlogged) {
            0 => warn!("audio stream error: {e}"),
            more => warn!("audio stream error: {e} (and {more} more since the last one told)"),
        }
        logged = Some(Instant::now());
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

    /// An input in bursts of `burst` frames, read a period of `period` at a time, the bursts
    /// and the reads out of step by turns: the underruns and the frames dropped.
    fn bursty(cushion: usize, burst: usize, period: usize) -> (u64, u64) {
        let (mut tx, rx) = rtrb::RingBuffer::<f32>::new(4096);
        let mut ring = InputRing { ring: rx, cushion, slack: None, primed: cushion == 0 };
        let stats = StreamStats::default();
        let mut into = vec![0.0; period];
        for step in 0..1000 {
            // a burst then a read, then the other way round: two reads in a row, and two bursts
            let order: &[bool] = if step % 2 == 0 { &[true, false] } else { &[false, true] };
            for &burst_comes in order {
                if burst_comes {
                    tx.write_chunk_uninit(burst).unwrap().fill_from_iter(std::iter::repeat(0.5));
                } else {
                    ring.read(&mut into, 1, &stats);
                }
            }
        }
        (stats.input_underruns.load(Ordering::Relaxed), stats.input_dropped.load(Ordering::Relaxed))
    }

    #[test]
    fn an_input_out_of_step_with_the_output_runs_short_without_a_cushion() {
        assert!(bursty(0, 48, 48).0 > 0);
    }

    #[test]
    fn a_cushion_of_a_burst_takes_the_steps_out() {
        assert_eq!(bursty(48, 48, 48), (0, 0));
    }

    #[test]
    fn the_input_starts_from_its_cushion_whatever_came_before() {
        let (mut tx, rx) = rtrb::RingBuffer::<f32>::new(4096);
        let mut ring = InputRing { ring: rx, cushion: 48, slack: Some(3 * 48), primed: false };
        let stats = StreamStats::default();
        tx.write_chunk_uninit(5 * 48).unwrap().fill_from_iter(std::iter::repeat(0.5));
        ring.read(&mut [0.0; 48], 1, &stats);
        assert_eq!(ring.ring.slots(), 48, "the cushion left");
        assert_eq!(stats.input_dropped.load(Ordering::Relaxed), 0, "a start drops nothing");
    }

    #[test]
    fn bursts_within_the_slack_are_kept() {
        let (mut tx, rx) = rtrb::RingBuffer::<f32>::new(4096);
        let mut ring = InputRing { ring: rx, cushion: 48, slack: Some(3 * 48), primed: false };
        let stats = StreamStats::default();
        let mut into = vec![0.0; 48];
        for step in 0..1000 {
            // now and then the reader is held up, and four bursts come before it reads four times
            let (bursts, reads) = match step % 10 {
                5 => (4, 0),
                6 => (0, 4),
                _ => (1, 1),
            };
            for _ in 0..bursts {
                tx.write_chunk_uninit(48).unwrap().fill_from_iter(std::iter::repeat(0.5));
            }
            for _ in 0..reads {
                ring.read(&mut into, 1, &stats);
            }
        }
        assert_eq!(stats.input_dropped.load(Ordering::Relaxed), 0);
        assert_eq!(stats.input_underruns.load(Ordering::Relaxed), 0);
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
            times_itself: true,
            hint: None,
            hinted: false,
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
