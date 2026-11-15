//! The USB sound cards Prisme drives itself, speaking the second USB audio class to them
//! through the system's access to the device: where the system's own audio reaches only some
//! of their channels (Android opens the inputs of a UMC404HD as eight mixed-up ones). The
//! system opens the card and hands its file over ([`UsbHost`]); Prisme streams it.

#[cfg_attr(not(any(target_os = "linux", target_os = "android")), allow(dead_code))]
mod descriptors;
#[cfg(any(target_os = "linux", target_os = "android"))]
mod driver;
#[cfg_attr(not(any(target_os = "linux", target_os = "android")), allow(dead_code))]
mod format;
#[cfg(any(target_os = "linux", target_os = "android"))]
mod usbfs;

use crate::devices::{Backend, DeviceInfo, DeviceKind};
use std::sync::{Mutex, OnceLock};

/// A card the system opened for Prisme: its file, and its descriptors (the device's, then its
/// configuration's).
pub struct OpenedCard {
    pub fd: i32,
    pub descriptors: Vec<u8>,
}

/// What the system does for the cards: says which are plugged in, opens one (asking the user
/// when it must, which may take a while), gives it back.
pub trait UsbHost: Send + Sync {
    fn cards(&self) -> Vec<String>;
    fn open(&self, name: &str) -> Option<OpenedCard>;
    fn close(&self, name: &str);
}

static HOST: OnceLock<Box<dyn UsbHost>> = OnceLock::new();

/// Gives the audio the system's way to its USB cards: [`Backend::Usb`] is offered from then on.
pub fn set_usb_host(host: Box<dyn UsbHost>) {
    let _ = HOST.set(host);
}

pub(crate) fn host() -> Option<&'static dyn UsbHost> {
    HOST.get().map(|h| h.as_ref())
}

/// The rate the cards run at: the one of the engine's other devices.
pub(crate) const SAMPLE_RATE: u32 = 48_000;

/// The channels of the cards once opened, outputs and inputs: listed before, a card says
/// nothing of them.
static CHANNELS: Mutex<Vec<(String, u16, u16)>> = Mutex::new(Vec::new());

/// The cards plugged in, each as an output and an input.
pub(crate) fn list() -> Vec<DeviceInfo> {
    let Some(host) = host() else { return Vec::new() };
    let known = CHANNELS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut list = Vec::new();
    for (n, name) in host.cards().into_iter().enumerate() {
        let (outputs, inputs) = known.iter().find(|(k, ..)| *k == name).map_or((0, 0), |&(_, o, i)| (o, i));
        for (kind, channels) in [(DeviceKind::Output, outputs), (DeviceKind::Input, inputs)] {
            list.push(DeviceInfo {
                name: name.clone(),
                kind,
                backend: Backend::Usb,
                channels,
                sample_rate: SAMPLE_RATE,
                buffer_range: None,
                is_default: n == 0,
            });
        }
    }
    list
}

#[cfg_attr(not(any(target_os = "linux", target_os = "android")), allow(dead_code))]
fn remember(name: &str, card: &descriptors::Card) {
    let channels = |s: Option<descriptors::Stream>| s.map_or(0, |s| s.channels as u16);
    let mut known = CHANNELS.lock().unwrap_or_else(|e| e.into_inner());
    known.retain(|(k, ..)| k != name);
    known.push((name.to_string(), channels(card.playback), channels(card.capture)));
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) use running::{start, Running};

#[cfg(any(target_os = "linux", target_os = "android"))]
mod running {
    use super::driver::{Driver, Options, MAX_URBS};
    use super::{descriptors, host, remember, SAMPLE_RATE};
    use crate::stream::{Renderer, RunningStream, StreamFormat, StreamSettings, StreamStats, Streams};
    use crate::{AudioCallback, AudioError};
    use std::sync::Arc;

    /// Transfers of a millisecond kept in flight each way when no period is asked for: a phone
    /// separating stems renders in under one, the third leaves room for its scheduling.
    const DEFAULT_URBS: usize = 3;

    /// A card streaming; dropping it stops it and gives the card back to the system.
    pub(crate) struct Running {
        driver: Option<Driver>,
        name: String,
    }

    impl Drop for Running {
        fn drop(&mut self) {
            self.driver = None;
            if let Some(host) = host() {
                host.close(&self.name);
            }
        }
    }

    /// Opens the card of `settings` (its output device, else the first plugged in), with its
    /// inputs if an input is asked for, and streams it through the callback.
    pub(crate) fn start<C: AudioCallback>(
        settings: &StreamSettings,
        make_callback: impl FnOnce(StreamFormat) -> C,
    ) -> Result<RunningStream, AudioError> {
        let host = host().ok_or(AudioError::BackendUnavailable(crate::Backend::Usb))?;
        let name = match settings.output_device.as_deref() {
            Some(wanted) => {
                host.cards().into_iter().find(|c| c.to_lowercase().contains(&wanted.to_lowercase()))
            }
            None => host.cards().into_iter().next(),
        }
        .ok_or_else(|| AudioError::DeviceNotFound(settings.output_device.clone().unwrap_or_default()))?;
        let opened =
            host.open(&name).ok_or_else(|| AudioError::Usb(format!("{name} could not be opened")))?;
        let mut running = Running { driver: None, name: name.clone() };
        let card = descriptors::parse(&opened.descriptors).map_err(|e| AudioError::Usb(e.to_string()))?;
        remember(&name, &card);
        let playback = card.playback.ok_or_else(|| AudioError::Usb(format!("{name} has no output")))?;
        let capture = card.capture.filter(|_| settings.input_device.is_some());

        let frames_per_ms = SAMPLE_RATE as usize / 1000;
        let urbs = settings
            .buffer_frames
            .map_or(DEFAULT_URBS, |f| (f as usize).div_ceil(frames_per_ms).clamp(2, MAX_URBS));
        let format = StreamFormat {
            sample_rate: SAMPLE_RATE,
            input_channels: capture.map_or(0, |c| c.channels),
            output_channels: playback.channels,
            buffer_frames: Some((urbs * frames_per_ms) as u32),
        };
        let stats = Arc::new(StreamStats::default());
        let (producer, ring) = match capture {
            Some(c) => {
                let (producer, ring) = rtrb::RingBuffer::<f32>::new(SAMPLE_RATE as usize * c.channels / 2);
                (Some(producer), Some(ring))
            }
            None => (None, None),
        };
        // the input comes a transfer at a time, out of step with the output's: one ahead, and as
        // many as may be in flight let wait (on the card's clock both ways, none is too many)
        let renderer = Renderer::new(make_callback(format), ring, format, stats.clone())
            .late_told_by_the_device()
            .input_cushion(frames_per_ms, MAX_URBS * frames_per_ms);
        let options = Options { sample_rate: SAMPLE_RATE, urbs, capture: capture.is_some() };
        running.driver = Some(
            Driver::start(opened.fd, &card, &options, renderer, producer, stats.clone())
                .map_err(|e| AudioError::Usb(format!("{name}: {e}")))?,
        );
        Ok(RunningStream::new(Streams::Usb { _card: running }, format, stats, name))
    }
}
