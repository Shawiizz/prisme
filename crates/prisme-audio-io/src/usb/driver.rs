//! Streams a card: its isochronous transfers submitted and reaped as they come back, a
//! millisecond of sound each. What the card captured goes to the ring the callback reads; each
//! output transfer reaped is filled again by the callback, as many frames as the card asks
//! (or brought in, when the outputs follow the inputs), so that its own clock paces Prisme.
//!
//! A thread waits for the transfers. On Android, where an application's own thread may be kept
//! waiting for the processor several milliseconds, it is the real-time thread of a silent
//! AAudio stream, borrowed at its first callback until the stream stops; elsewhere, or if
//! Android refuses that stream, one of the driver's own.
//!
//! The card holds a few transfers: when they come back later than they last, the card ran
//! out of sound, which counts as a late period; it then holds one more, up to [`MAX_URBS`],
//! and one less again after a while without (down to as many as at the start).

use super::descriptors::{Card, Stream};
use super::format::{decode, encode, PacketSizer};
use super::usbfs::{self, Urb};
use crate::stream::{Renderer, StreamStats};
use crate::AudioCallback;
use std::io;
use std::os::fd::RawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tracing::warn;

/// The audio class's request to set a control, and its sampling frequency control.
const SET_CURRENT: u8 = 0x01;
const GET_CURRENT: u8 = 0x01;
const SAMPLING_FREQUENCY: u16 = 0x0100;
/// Host to interface, class request; and back.
const TO_INTERFACE: u8 = 0x21;
const FROM_INTERFACE: u8 = 0xa1;
const CONTROL_TIMEOUT: Duration = Duration::from_secs(1);

/// What a transfer is, in the high half of its context.
const PLAYBACK: usize = 1;
const CAPTURE: usize = 2;
const FEEDBACK: usize = 3;
/// Feedback transfers kept in flight.
const FEEDBACK_URBS: usize = 2;
/// Transfers a way holds at most, as late periods make it hold more.
pub(crate) const MAX_URBS: usize = 8;
/// The first moments of a stream, where the system is still busy starting it all and nothing
/// is played yet: a late period then is not one.
const WARM_UP: Duration = Duration::from_secs(2);
/// How long the transfers keep time before a transfer less is held.
const CALM: Duration = Duration::from_secs(30);
/// How long a stop waits for the transfers to come back by themselves.
const STOP_WAIT: Duration = Duration::from_millis(300);

/// A card streaming. Dropping it stops the stream and puts the card's interfaces back at rest.
pub(crate) struct Driver {
    fd: RawFd,
    stop: Arc<AtomicBool>,
    stopped: mpsc::Receiver<()>,
    /// Where the transfers lie, to take them back from the kernel when they do not return.
    urbs: Vec<usize>,
    reaper: Reaper,
}

/// The thread waiting for the transfers.
enum Reaper {
    #[cfg(target_os = "android")]
    Helper {
        _stream: Helper,
    },
    Thread(Option<JoinHandle<()>>),
}

pub(crate) struct Options {
    pub sample_rate: u32,
    /// Transfers of a millisecond each kept in flight on each way, to begin with.
    pub urbs: usize,
    pub capture: bool,
}

impl Driver {
    /// Sets the card's clock to `options.sample_rate`, its streams to their settings, and
    /// starts streaming them.
    pub fn start<C: AudioCallback>(
        fd: RawFd,
        card: &Card,
        options: &Options,
        renderer: Renderer<C>,
        captured: Option<rtrb::Producer<f32>>,
        stats: Arc<StreamStats>,
    ) -> io::Result<Driver> {
        let playback = card.playback.ok_or_else(|| io::Error::other("the card has no output"))?;
        let capture = card.capture.filter(|_| options.capture);
        let high_speed = usbfs::high_speed(fd)?;
        set_rate(fd, card.control, playback.clock, options.sample_rate)?;
        if let Some(c) = capture.filter(|c| c.clock != playback.clock) {
            set_rate(fd, card.control, c.clock, options.sample_rate)?;
        }
        for s in [Some(playback), capture].into_iter().flatten() {
            usbfs::set_interface(fd, s.interface, s.alt)?;
        }

        let per_second = |s: &Stream| packets_per_second(s.interval, high_speed);
        let packets = |s: &Stream| (per_second(s) / 1000).max(1) as usize;
        let max_frames = playback.max_packet / playback.frame_bytes();
        let side = |s: &Stream, role: usize| {
            (0..MAX_URBS)
                .map(|i| Urb::new(s.endpoint, packets(s), s.max_packet, role << 16 | i))
                .collect::<Vec<_>>()
        };
        let playback_urbs = side(&playback, PLAYBACK);
        let capture_urbs = capture.as_ref().map(|c| side(c, CAPTURE)).unwrap_or_default();
        // the inputs on the clock of the outputs, at their pace: the outputs follow them, as
        // Linux does for the UMC404HD, whose feedback endpoint is not to be trusted
        let follows_inputs =
            capture.is_some_and(|c| c.clock == playback.clock && per_second(&c) == per_second(&playback));
        let feedback_urbs: Vec<Urb> = playback
            .feedback
            .filter(|_| !follows_inputs)
            .map(|endpoint| {
                (0..FEEDBACK_URBS).map(|i| Urb::new(endpoint, 1, 4, FEEDBACK << 16 | i)).collect()
            })
            .unwrap_or_default();

        let now = Instant::now();
        let mut state = Box::new(State {
            fd,
            sizer: PacketSizer::new(options.sample_rate, per_second(&playback), max_frames),
            scratch: vec![0.0; scratch_samples(&playback, packets(&playback), capture.as_ref())],
            renderer,
            captured,
            playback,
            capture,
            follows_inputs,
            urbs: options.urbs.clamp(2, MAX_URBS),
            least_urbs: options.urbs.clamp(2, MAX_URBS),
            urb_span: Duration::from_secs_f64(packets(&playback) as f64 / f64::from(per_second(&playback))),
            playback_urbs,
            capture_urbs,
            feedback_urbs,
            pending: 0,
            lost: false,
            started: now,
            last_played: None,
            calm_since: now,
            stats,
        });
        let mut urbs = Vec::new();
        for list in [&mut state.playback_urbs, &mut state.capture_urbs, &mut state.feedback_urbs] {
            urbs.extend(list.iter_mut().map(|u| u.address()));
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (done, stopped) = mpsc::channel();
        let run = {
            let stop = stop.clone();
            move |mut state: Box<State<C>>| {
                state.run(&stop);
                let _ = done.send(());
            }
        };
        #[cfg(target_os = "android")]
        let (state, run) = match helper(state, run) {
            Ok(helper) => {
                return Ok(Driver { fd, stop, stopped, urbs, reaper: Reaper::Helper { _stream: helper } })
            }
            Err(back) => back,
        };
        let thread = std::thread::Builder::new().name("prisme-usb-audio".into()).spawn(move || {
            raise_priority();
            run(state);
        })?;
        Ok(Driver { fd, stop, stopped, urbs, reaper: Reaper::Thread(Some(thread)) })
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if self.stopped.recv_timeout(STOP_WAIT).is_err() {
            for &urb in &self.urbs {
                usbfs::discard(self.fd, urb);
            }
        }
        if let Reaper::Thread(thread) = &mut self.reaper {
            if let Some(thread) = thread.take() {
                let _ = thread.join();
            }
        }
    }
}

/// A silent AAudio stream lending its real-time thread to `run`, from its first callback until
/// `run` returns: Android gives an application none of its own. Both back if Android refuses
/// the stream.
#[cfg(target_os = "android")]
fn helper<C: AudioCallback, R: FnOnce(Box<State<C>>) + Send + 'static>(
    state: Box<State<C>>,
    run: R,
) -> Result<Helper, (Box<State<C>>, R)> {
    use ndk::audio::{
        AudioCallbackResult, AudioDirection, AudioFormat, AudioPerformanceMode, AudioSharingMode,
        AudioStreamBuilder,
    };
    const CHANNELS: usize = 2;
    // taken by the callback at its first call, or back here if the stream does not start
    let slot = Arc::new(Mutex::new(Some((state, run))));
    let callback = {
        let slot = slot.clone();
        move |_: &ndk::audio::AudioStream, data: *mut std::ffi::c_void, frames: i32| {
            // SAFETY: AAudio hands a buffer of `frames` frames of the format asked for
            unsafe { std::ptr::write_bytes(data.cast::<i16>(), 0, frames.max(0) as usize * CHANNELS) };
            let taken = slot.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some((state, run)) = taken {
                run(state);
            }
            AudioCallbackResult::Stop
        }
    };
    let stream = AudioStreamBuilder::new()
        .map(|b| {
            b.direction(AudioDirection::Output)
                .performance_mode(AudioPerformanceMode::LowLatency)
                .sharing_mode(AudioSharingMode::Shared)
                .format(AudioFormat::PCM_I16)
                .channel_count(CHANNELS as i32)
                .data_callback(Box::new(callback))
        })
        .and_then(|b| b.open_stream())
        .and_then(|stream| stream.request_start().map(|()| stream));
    match stream {
        Ok(stream) => Ok(Helper { _stream: stream }),
        Err(e) => {
            warn!("the USB card goes without a helper: {e}");
            Err(slot
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
                .expect("a stream that did not start never called back"))
        }
    }
}

/// The silent stream of [`helper`], closed when dropped.
#[cfg(target_os = "android")]
struct Helper {
    _stream: ndk::audio::AudioStream,
}

// SAFETY: an AAudio stream may be closed from any thread but its own callback's.
#[cfg(target_os = "android")]
unsafe impl Send for Helper {}

/// Packets a second of an endpoint polled every `interval`.
fn packets_per_second(interval: u8, high_speed: bool) -> u32 {
    if high_speed {
        8_000 >> interval.clamp(1, 4).saturating_sub(1)
    } else {
        1_000 / u32::from(interval.max(1))
    }
}

/// The floats a transfer's samples take at most: an output transfer's, a captured packet's.
fn scratch_samples(playback: &Stream, packets: usize, capture: Option<&Stream>) -> usize {
    let output = packets * playback.max_packet / playback.frame_bytes() * playback.channels;
    output.max(capture.map_or(0, |c| c.max_packet / c.subslot))
}

/// Sets clock source `clock` to `rate`, and checks the card runs at it (a fixed clock refuses
/// to be set, and may run at it already).
fn set_rate(fd: RawFd, control: u8, clock: u8, rate: u32) -> io::Result<()> {
    let index = u16::from(clock) << 8 | u16::from(control);
    let mut bytes = rate.to_le_bytes();
    if let Err(e) =
        usbfs::control(fd, TO_INTERFACE, SET_CURRENT, SAMPLING_FREQUENCY, index, &mut bytes, CONTROL_TIMEOUT)
    {
        warn!("the card's clock could not be set to {rate} Hz: {e}");
    }
    let mut now = [0u8; 4];
    usbfs::control(fd, FROM_INTERFACE, GET_CURRENT, SAMPLING_FREQUENCY, index, &mut now, CONTROL_TIMEOUT)?;
    match u32::from_le_bytes(now) {
        r if r == rate => Ok(()),
        r => Err(io::Error::other(format!("the card runs at {r} Hz, not {rate} Hz"))),
    }
}

/// The stream's transfers and what they carry.
struct State<C> {
    fd: RawFd,
    renderer: Renderer<C>,
    captured: Option<rtrb::Producer<f32>>,
    playback: Stream,
    capture: Option<Stream>,
    /// The outputs carry as many frames as the inputs brought, packet for packet.
    follows_inputs: bool,
    /// Transfers held in flight each way, as many as at the start at least, and how long an
    /// output one lasts.
    urbs: usize,
    least_urbs: usize,
    urb_span: Duration,
    playback_urbs: Vec<Urb>,
    capture_urbs: Vec<Urb>,
    feedback_urbs: Vec<Urb>,
    /// Transfers with the kernel.
    pending: usize,
    /// The card went away.
    lost: bool,
    started: Instant,
    /// When an output transfer last came back, and since when none came back late.
    last_played: Option<Instant>,
    calm_since: Instant,
    sizer: PacketSizer,
    /// The samples of a transfer, as floats.
    scratch: Vec<f32>,
    stats: Arc<StreamStats>,
}

impl<C: AudioCallback> State<C> {
    /// Submits the transfers to begin with: the outputs first, some cards (a UMC404HD) want
    /// them going before the inputs.
    fn prime(&mut self) {
        let capture = if self.capture.is_some() { self.urbs } else { 0 };
        let all = [(PLAYBACK, self.urbs), (CAPTURE, capture), (FEEDBACK, self.feedback_urbs.len())];
        for (role, count) in all {
            for i in 0..count {
                if let Err(e) = self.start_urb(role, i) {
                    warn!("the USB card refused a transfer: {e}");
                    self.lost = true;
                }
            }
        }
    }

    /// Streams until `stop`, or the card gone: waits for each transfer to come back, then reaps
    /// those back with it. Gone, the card's interfaces are back at rest.
    fn run(&mut self, stop: &AtomicBool) {
        if !stop.load(Ordering::Relaxed) {
            self.prime();
        }
        while self.pending > 0 && !self.lost {
            match usbfs::reap(self.fd) {
                Ok(context) => {
                    let stop = stop.load(Ordering::Relaxed);
                    self.complete(context, stop);
                    self.drain(stop);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => {
                    // the device went away: the transfers with it
                    warn!("the USB card stopped answering: {e}");
                    self.lost = true;
                }
            }
        }
    }

    /// Reaps the transfers back already, without waiting.
    fn drain(&mut self, stop: bool) {
        while !self.lost {
            match usbfs::reap_now(self.fd) {
                Ok(Some(context)) => self.complete(context, stop),
                Ok(None) => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => self.lost = true,
            }
        }
    }

    /// A transfer came back: what it brought taken, it is filled again and submitted, unless
    /// the stream stops or holds fewer.
    fn complete(&mut self, context: usize, stop: bool) {
        self.pending -= 1;
        let (role, i) = (context >> 16, context & 0xffff);
        self.urb(role, i).in_flight = false;
        if role == PLAYBACK && !stop {
            self.keep_time();
        }
        if is_gone(self.urb(role, i).status()) {
            self.lost = true;
        }
        if self.lost || stop {
            return;
        }
        // a transfer above those held now is not submitted again
        let retired = role != FEEDBACK && i >= self.urbs;
        match role {
            CAPTURE => self.captured(i),
            FEEDBACK => {
                let (packets, data) = self.feedback_urbs[i].received();
                if packets[0].status == 0 {
                    self.sizer.feedback(&data[..packets[0].actual_length as usize]);
                }
            }
            _ if retired => {}
            _ => self.fill(i),
        }
        if !retired && self.submit(role, i).is_err() {
            self.lost = true;
        }
    }

    /// An output transfer came back: later than all those held last, the card ran out of sound
    /// meanwhile, and holds one more; after a while without, one less.
    fn keep_time(&mut self) {
        let now = Instant::now();
        let ran_out = self.last_played.is_some_and(|at| now - at > self.urb_span * self.urbs as u32);
        if ran_out && now - self.started > WARM_UP {
            self.stats.over_budget.fetch_add(1, Ordering::Relaxed);
            self.hold_one_more();
            self.calm_since = now;
        } else if now - self.calm_since > CALM && self.urbs > self.least_urbs {
            // the highest transfers are not submitted again as they come back
            self.urbs -= 1;
            self.calm_since = now;
        }
        self.last_played = Some(now);
    }

    /// Fills transfer `i` if it is an output, and submits it.
    fn start_urb(&mut self, role: usize, i: usize) -> io::Result<()> {
        if role == PLAYBACK {
            self.fill(i);
        }
        self.submit(role, i)
    }

    /// After the card ran out of sound: the inputs taken up again from now, and one more
    /// transfer held each way, while there is room for it.
    fn hold_one_more(&mut self) {
        self.sizer.realign();
        self.renderer.resync_input();
        if self.urbs == MAX_URBS {
            return;
        }
        let i = self.urbs;
        self.urbs += 1;
        let capture = self.capture.is_some();
        // one retired not back yet is held again as it comes back
        for (role, used) in [(PLAYBACK, true), (CAPTURE, capture)] {
            if used && !self.urb(role, i).in_flight && self.start_urb(role, i).is_err() {
                self.lost = true;
            }
        }
    }

    fn urb(&mut self, role: usize, i: usize) -> &mut Urb {
        match role {
            CAPTURE => &mut self.capture_urbs[i],
            FEEDBACK => &mut self.feedback_urbs[i],
            _ => &mut self.playback_urbs[i],
        }
    }

    fn submit(&mut self, role: usize, i: usize) -> io::Result<()> {
        let fd = self.fd;
        let urb = self.urb(role, i);
        // SAFETY: the transfers live in this state, which waits for every one submitted to
        // come back before it goes (or the device is gone, and with it the kernel's hold on
        // them).
        unsafe { urb.submit(fd) }?;
        urb.in_flight = true;
        self.pending += 1;
        Ok(())
    }

    /// Fills output transfer `i` with the frames the card asks for, from the callback.
    fn fill(&mut self, i: usize) {
        let p = self.playback;
        let urb = &mut self.playback_urbs[i];
        let mut frames = [0usize; 64];
        let packets = urb.packets_mut().len().min(frames.len());
        for f in &mut frames[..packets] {
            *f = self.sizer.next();
        }
        let total: usize = frames[..packets].iter().sum();
        let samples = &mut self.scratch[..total * p.channels];
        self.renderer.render(samples);
        encode(samples, p.subslot, &mut urb.data_mut()[..total * p.frame_bytes()]);
        urb.set_lengths(frames[..packets].iter().map(|f| f * p.frame_bytes()));
    }

    /// Hands what capture transfer `i` brought to the callback's ring, and its pace to the
    /// outputs that follow it.
    fn captured(&mut self, i: usize) {
        let (Some(c), Some(ring)) = (self.capture, self.captured.as_mut()) else { return };
        let (packets, data) = self.capture_urbs[i].received();
        for (n, packet) in packets.iter().enumerate() {
            if packet.status != 0 {
                continue;
            }
            let bytes = packet.actual_length as usize / c.frame_bytes() * c.frame_bytes();
            if self.follows_inputs {
                self.sizer.captured(bytes / c.frame_bytes());
            }
            let from = n * c.max_packet;
            let samples = &mut self.scratch[..bytes / c.subslot];
            decode(&data[from..from + bytes], c.subslot, samples);
            let room = samples.len().min(ring.slots());
            if let Ok(chunk) = ring.write_chunk_uninit(room) {
                chunk.fill_from_iter(samples[..room].iter().copied());
            }
            if room < samples.len() {
                self.stats
                    .input_dropped
                    .fetch_add(((samples.len() - room) / c.channels) as u64, Ordering::Relaxed);
            }
        }
    }
}

impl<C> Drop for State<C> {
    /// The transfers the kernel still holds taken back before they go, the card's interfaces
    /// back at rest, and the card told gone if it is.
    fn drop(&mut self) {
        if self.lost {
            self.stats.device_lost.store(true, Ordering::Relaxed);
            return;
        }
        for list in [&mut self.playback_urbs, &mut self.capture_urbs, &mut self.feedback_urbs] {
            for urb in list.iter_mut().filter(|u| u.in_flight) {
                usbfs::discard(self.fd, urb.address());
            }
        }
        while self.pending > 0 {
            match usbfs::reap(self.fd) {
                Ok(_) => self.pending -= 1,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        for s in [Some(self.playback), self.capture].into_iter().flatten() {
            let _ = usbfs::set_interface(self.fd, s.interface, 0);
        }
    }
}

/// A transfer's status saying the device is gone.
fn is_gone(status: i32) -> bool {
    matches!(-status, libc::ENODEV | libc::ESHUTDOWN)
}

/// The streaming thread as urgent as audio may be for an application (Android lets it ask for
/// this much). Left to go to any core: its work is light, and kept to the fastest it would
/// wait for them while they draw the screen.
fn raise_priority() {
    const URGENT_AUDIO: i32 = -19;
    // SAFETY: on Linux, 0 is the calling thread.
    if unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, URGENT_AUDIO) } != 0 {
        warn!("the USB audio thread kept its priority: {}", io::Error::last_os_error());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scratch_holds_an_output_transfer_and_a_captured_packet() {
        let stream = |max_packet, subslot| Stream {
            interface: 1,
            alt: 1,
            channels: 4,
            subslot,
            bits: 24,
            endpoint: 1,
            max_packet,
            interval: 1,
            feedback: None,
            clock: 0x29,
        };
        // a UMC404HD: 25 frames of 4 channels a packet, 8 packets
        assert_eq!(scratch_samples(&stream(400, 4), 8, Some(&stream(400, 4))), 800);
        assert_eq!(scratch_samples(&stream(16, 4), 1, Some(&stream(1024, 2))), 512);
    }

    #[test]
    fn high_speed_endpoints_poll_by_microframes() {
        assert_eq!(packets_per_second(1, true), 8_000);
        assert_eq!(packets_per_second(4, true), 1_000);
        assert_eq!(packets_per_second(1, false), 1_000);
    }
}
