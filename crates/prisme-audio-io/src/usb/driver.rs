//! Streams a card: a thread submits its isochronous transfers and reaps them as they come
//! back, a millisecond of sound each. What the card captured goes to the ring the callback
//! reads; each output transfer reaped is filled again by the callback, as many frames as the
//! card asks through its feedback endpoint, so that its own clock paces Prisme. The card
//! holds a few of them: one filled too late is heard only if it ran out of all, which counts
//! as a late period.

use super::descriptors::{Card, Stream};
use super::format::{decode, encode, PacketSizer};
use super::usbfs::{self, Urb};
use crate::stream::{Renderer, StreamStats};
use crate::AudioCallback;
use std::io;
use std::os::fd::RawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;
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
/// How long a stop waits for the transfers to come back by themselves.
const STOP_WAIT: Duration = Duration::from_millis(300);

/// A card streaming. Dropping it stops the stream and puts the card's interfaces back at rest.
pub(crate) struct Driver {
    fd: RawFd,
    stop: Arc<AtomicBool>,
    stopped: mpsc::Receiver<()>,
    thread: Option<JoinHandle<()>>,
    /// Where the transfers lie, to take them back from the kernel when they do not return.
    urbs: Vec<usize>,
}

pub(crate) struct Options {
    pub sample_rate: u32,
    /// Transfers of a millisecond each kept in flight on each way.
    pub urbs: usize,
    pub capture: bool,
}

impl Driver {
    /// Sets the card's clock to `options.sample_rate`, its streams to their settings, and
    /// starts the thread that streams them.
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
            (0..options.urbs.max(2))
                .map(|i| Urb::new(s.endpoint, packets(s), s.max_packet, role << 16 | i))
                .collect::<Vec<_>>()
        };
        let playback_urbs = side(&playback, PLAYBACK);
        let capture_urbs = capture.as_ref().map(|c| side(c, CAPTURE)).unwrap_or_default();
        let feedback_urbs: Vec<Urb> = playback
            .feedback
            .map(|endpoint| {
                (0..FEEDBACK_URBS).map(|i| Urb::new(endpoint, 1, 4, FEEDBACK << 16 | i)).collect()
            })
            .unwrap_or_default();

        let mut state = State {
            fd,
            sizer: PacketSizer::new(options.sample_rate, per_second(&playback), max_frames),
            scratch: vec![0.0; scratch_samples(&playback, packets(&playback), capture.as_ref())],
            renderer,
            captured,
            playback,
            capture,
            playback_urbs,
            capture_urbs,
            feedback_urbs,
            stats,
        };
        let mut urbs = Vec::new();
        for list in [&mut state.playback_urbs, &mut state.capture_urbs, &mut state.feedback_urbs] {
            urbs.extend(list.iter_mut().map(|u| u.address()));
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (done, stopped) = mpsc::channel();
        let thread = std::thread::Builder::new().name("prisme-usb-audio".into()).spawn({
            let stop = stop.clone();
            move || {
                state.run(&stop);
                let _ = done.send(());
            }
        })?;
        Ok(Driver { fd, stop, stopped, thread: Some(thread), urbs })
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
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

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

/// The streaming thread's own.
struct State<C> {
    fd: RawFd,
    renderer: Renderer<C>,
    captured: Option<rtrb::Producer<f32>>,
    playback: Stream,
    capture: Option<Stream>,
    playback_urbs: Vec<Urb>,
    capture_urbs: Vec<Urb>,
    feedback_urbs: Vec<Urb>,
    sizer: PacketSizer,
    /// The samples of a transfer, as floats.
    scratch: Vec<f32>,
    stats: Arc<StreamStats>,
}

impl<C: AudioCallback> State<C> {
    fn run(&mut self, stop: &AtomicBool) {
        raise_priority();
        let mut pending = 0;
        let mut playing = 0;
        let mut lost = false;
        let all = [
            (CAPTURE, self.capture_urbs.len()),
            (FEEDBACK, self.feedback_urbs.len()),
            (PLAYBACK, self.playback_urbs.len()),
        ];
        for (role, count) in all {
            for i in 0..count {
                if role == PLAYBACK {
                    self.fill(i);
                }
                match self.submit(role, i) {
                    Ok(()) => {
                        pending += 1;
                        playing += usize::from(role == PLAYBACK);
                    }
                    Err(e) => {
                        warn!("the USB card refused a transfer: {e}");
                        lost = true;
                    }
                }
            }
        }
        while pending > 0 {
            let context = match usbfs::reap(self.fd) {
                Ok(context) => context,
                Err(e) => {
                    // the device went away: the transfers with it
                    warn!("the USB card stopped answering: {e}");
                    lost = true;
                    break;
                }
            };
            pending -= 1;
            let (role, i) = (context >> 16, context & 0xffff);
            if role == PLAYBACK {
                playing -= 1;
                if playing == 0 && !stop.load(Ordering::Relaxed) {
                    // the card played all it was given before the next came
                    self.stats.over_budget.fetch_add(1, Ordering::Relaxed);
                }
            }
            let status = self.urb(role, i).status();
            if is_gone(status) {
                lost = true;
            }
            if lost || stop.load(Ordering::Relaxed) {
                continue;
            }
            match role {
                CAPTURE => self.captured(i),
                FEEDBACK => {
                    let (packets, data) = self.feedback_urbs[i].received();
                    if packets[0].status == 0 {
                        self.sizer.feedback(&data[..packets[0].actual_length as usize]);
                    }
                }
                _ => self.fill(i),
            }
            if self.submit(role, i).is_err() {
                lost = true;
            } else {
                pending += 1;
                playing += usize::from(role == PLAYBACK);
            }
        }
        if lost {
            self.stats.device_lost.store(true, Ordering::Relaxed);
        }
        for s in [Some(self.playback), self.capture].into_iter().flatten() {
            let _ = usbfs::set_interface(self.fd, s.interface, 0);
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
        // SAFETY: the transfers live in this state until the thread ends, which it does only
        // once every one submitted was reaped (or the device is gone, and with it the kernel's
        // hold on them).
        unsafe { self.urb(role, i).submit(fd) }
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

    /// Hands what capture transfer `i` brought to the callback's ring.
    fn captured(&mut self, i: usize) {
        let (Some(c), Some(ring)) = (self.capture, self.captured.as_mut()) else { return };
        let (packets, data) = self.capture_urbs[i].received();
        for (n, packet) in packets.iter().enumerate() {
            if packet.status != 0 {
                continue;
            }
            let bytes = packet.actual_length as usize / c.frame_bytes() * c.frame_bytes();
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

/// A transfer's status saying the device is gone.
fn is_gone(status: i32) -> bool {
    matches!(-status, libc::ENODEV | libc::ESHUTDOWN)
}

/// The streaming thread as urgent as audio may be for an application (Android lets it ask
/// for this much), on the fastest cores: on a small one a period takes several times as long.
fn raise_priority() {
    const URGENT_AUDIO: i32 = -19;
    // SAFETY: on Linux, 0 is the calling thread.
    if unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, URGENT_AUDIO) } != 0 {
        warn!("the USB audio thread kept its priority: {}", io::Error::last_os_error());
    }
    let fast = fast_cores();
    if fast.is_empty() {
        return;
    }
    // SAFETY: a set of cores filled here, for the calling thread (0).
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        fast.iter().for_each(|&core| libc::CPU_SET(core, &mut set));
        // the cores of an application out of sight may be others: it keeps them then
        libc::sched_setaffinity(0, size_of::<libc::cpu_set_t>(), &set);
    }
}

/// More cores than a phone has.
const MAX_CORES: usize = 64;

/// The cores at least three quarters as fast as the fastest, by their top frequency.
fn fast_cores() -> Vec<usize> {
    let top = |core: usize| {
        std::fs::read_to_string(format!("/sys/devices/system/cpu/cpu{core}/cpufreq/cpuinfo_max_freq"))
            .ok()
            .and_then(|f| f.trim().parse::<u64>().ok())
    };
    let cores: Vec<(usize, u64)> = (0..MAX_CORES).map_while(|c| Some((c, top(c)?))).collect();
    let fastest = cores.iter().map(|&(_, f)| f).max().unwrap_or(0);
    cores.into_iter().filter(|&(_, f)| f * 4 >= fastest * 3).map(|(c, _)| c).collect()
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
