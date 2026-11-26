//! The audio: its settings, the devices, the audio running and how well it keeps up.

use crate::state::{lock, AppState};
use prisme_audio_io::{usb_asks, Backend, DeviceRef, StreamStats};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::atomic::Ordering;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "AudioDevice"))]
pub struct DeviceDto {
    /// What its system calls it, by which it is chosen ([`DeviceChoice`]).
    pub id: String,
    pub name: String,
    /// The sound card it is a side of: an interface's outputs and inputs share it.
    pub card: String,
    /// "input" or "output"
    pub kind: &'static str,
    pub channels: u16,
    pub sample_rate: u32,
    /// The periods the settings offer that the driver accepts.
    pub buffer_sizes: Vec<BufferSizeDto>,
    pub is_default: bool,
}

/// The audio settings drafted in the settings, as the backend shapes them.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "AudioDraft"))]
pub struct AudioDraftDto {
    pub draft: AudioSettings,
    /// It differs from the settings kept: Apply restarts the audio.
    pub changed: bool,
    /// The periods offered: the output chosen's, else the default output's, else every one at
    /// 48 kHz.
    pub buffer_sizes: Vec<BufferSizeDto>,
}

impl AudioDraftDto {
    /// `draft` against the settings `kept`, `devices` those of its driver.
    pub fn of(draft: AudioSettings, kept: &AudioSettings, devices: &[DeviceDto]) -> Self {
        let outputs = || devices.iter().filter(|d| d.kind == "output");
        let output = outputs()
            .find(|d| draft.output.as_ref().is_some_and(|chosen| chosen.id == d.id))
            .or_else(|| outputs().find(|d| d.is_default));
        let buffer_sizes = output.map_or_else(|| BufferSizeDto::offered(None, 0), |d| d.buffer_sizes.clone());
        Self { changed: draft != *kept, draft, buffer_sizes }
    }
}

/// A period of the audio: so many frames, so long at the device's rate.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "BufferSize"))]
pub struct BufferSizeDto {
    pub frames: u32,
    pub ms: f64,
}

impl BufferSizeDto {
    /// The periods the settings offer (frames).
    const OFFERED: [u32; 7] = [32, 64, 128, 256, 512, 1024, 2048];

    /// Those of [`Self::OFFERED`] within `range` (all if the driver does not say), at `rate`
    /// (48 kHz if it does not say).
    pub fn offered(range: Option<(u32, u32)>, rate: u32) -> Vec<Self> {
        let rate = if rate == 0 { 48_000 } else { rate };
        Self::OFFERED
            .into_iter()
            .filter(|n| range.is_none_or(|(min, max)| (min..=max).contains(n)))
            .map(|frames| Self { frames, ms: f64::from(frames) * 1000.0 / f64::from(rate) })
            .collect()
    }
}

/// An audio driver the settings offer.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "BackendChoice"))]
pub struct BackendChoiceDto {
    pub backend: BackendDto,
    /// One device for the outputs and, with turntables on it, the inputs (an ASIO driver, a USB
    /// card Prisme drives itself).
    pub one_device: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "AudioBackend"))]
pub enum BackendDto {
    Wasapi,
    Asio,
    CoreAudio,
    Alsa,
    Jack,
    Aaudio,
    Usb,
}

impl Default for BackendDto {
    fn default() -> Self {
        Backend::SYSTEM.into()
    }
}

impl BackendDto {
    /// Whether it reaches one device for the outputs and the inputs.
    pub fn one_device(self) -> bool {
        matches!(self, BackendDto::Asio | BackendDto::Usb)
    }
}

impl fmt::Display for BackendDto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Backend::from(*self).fmt(f)
    }
}

impl From<BackendDto> for Backend {
    fn from(b: BackendDto) -> Self {
        match b {
            BackendDto::Wasapi => Backend::Wasapi,
            BackendDto::Asio => Backend::Asio,
            BackendDto::CoreAudio => Backend::CoreAudio,
            BackendDto::Alsa => Backend::Alsa,
            BackendDto::Jack => Backend::Jack,
            BackendDto::Aaudio => Backend::AAudio,
            BackendDto::Usb => Backend::Usb,
        }
    }
}

impl From<Backend> for BackendDto {
    fn from(b: Backend) -> Self {
        match b {
            Backend::Wasapi => BackendDto::Wasapi,
            Backend::Asio => BackendDto::Asio,
            Backend::CoreAudio => BackendDto::CoreAudio,
            Backend::Alsa => BackendDto::Alsa,
            Backend::Jack => BackendDto::Jack,
            Backend::AAudio => BackendDto::Aaudio,
            Backend::Usb => BackendDto::Usb,
        }
    }
}

/// A device chosen in the settings: the id its system gives it, by which it is found again, and
/// its name, shown, and by which the same model is found when its id is not there (see
/// [`DeviceRef`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "DeviceChoice"))]
pub struct DeviceChoice {
    pub id: String,
    pub name: String,
}

impl From<&DeviceDto> for DeviceChoice {
    fn from(device: &DeviceDto) -> Self {
        Self { id: device.id.clone(), name: device.name.clone() }
    }
}

#[cfg(test)]
impl From<&str> for DeviceChoice {
    fn from(name: &str) -> Self {
        Self { id: name.to_owned(), name: name.to_owned() }
    }
}

impl From<&DeviceChoice> for DeviceRef {
    fn from(choice: &DeviceChoice) -> Self {
        Self { id: choice.id.clone(), name: choice.name.clone() }
    }
}

/// Audio settings chosen by the user.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AudioSettings {
    pub backend: BackendDto,
    pub output: Option<DeviceChoice>,
    /// Turntable input (DVS interface). None = no turntables.
    pub input: Option<DeviceChoice>,
    /// Feed deck 1 with a simulated turntable instead of the input.
    pub simulate: bool,
    pub buffer_frames: Option<u32>,
    /// The record of every turntable, in files from before each had its own (see
    /// [`Preferences::settle`](crate::preferences::Preferences::settle)).
    #[serde(rename = "timecodeFormat", skip_serializing)]
    #[cfg_attr(test, ts(skip))]
    pub(crate) timecode_before: Option<String>,
}

/// A change of the audio settings, drafted in the settings window until applied.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", tag = "setting", content = "value")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "AudioChange"))]
pub enum AudioChangeDto {
    /// Another driver has other devices: its defaults, its buffer size.
    Backend(BackendDto),
    /// The one device of a driver that has one for the outputs and the inputs (`oneDevice`):
    /// its turntables, if they are on, on it too.
    Driver(Option<DeviceChoice>),
    Output(Option<DeviceChoice>),
    Input(Option<DeviceChoice>),
    /// The turntables on the one device of the driver, or none.
    TurntablesOnDriver(bool),
    Buffer(Option<u32>),
    Simulate(bool),
}

impl AudioSettings {
    /// These settings with `change`; the turntables put on a driver with no device chosen go on
    /// its `first` output device.
    pub fn changed(&self, change: AudioChangeDto, first: impl FnOnce() -> Option<DeviceChoice>) -> Self {
        let mut next = self.clone();
        match change {
            AudioChangeDto::Backend(backend) => {
                next = Self { backend, output: None, input: None, buffer_frames: None, ..next }
            }
            AudioChangeDto::Driver(device) => {
                next.input = next.input.and(device.clone());
                next.output = device;
            }
            AudioChangeDto::Output(device) => next.output = device,
            AudioChangeDto::Input(device) => next.input = device,
            AudioChangeDto::TurntablesOnDriver(on) => {
                next.input = if on { next.output.clone().or_else(first) } else { None }
            }
            AudioChangeDto::Buffer(frames) => next.buffer_frames = frames,
            AudioChangeDto::Simulate(on) => next.simulate = on,
        }
        next
    }

    /// These settings with the turntables on, `devices` those of their driver: on its one
    /// device if it has one (as [`AudioChangeDto::TurntablesOnDriver`]), else on the inputs of
    /// the output's sound card, or its default inputs. None if the driver has no input.
    pub fn with_turntables(&self, devices: &[DeviceDto]) -> Option<Self> {
        let of_kind = |kind| devices.iter().filter(move |d| d.kind == kind);
        if self.backend.one_device() {
            let first = || of_kind("output").next().map(DeviceChoice::from);
            return Some(self.changed(AudioChangeDto::TurntablesOnDriver(true), first))
                .filter(|on| on.input.is_some());
        }
        let output = match &self.output {
            Some(chosen) => of_kind("output").find(|d| chosen.id == d.id),
            None => of_kind("output").find(|d| d.is_default),
        };
        let input = of_kind("input")
            .find(|d| output.is_some_and(|o| o.card == d.card))
            .or_else(|| of_kind("input").find(|d| d.is_default))
            .or_else(|| of_kind("input").next())?;
        Some(Self { input: Some(input.into()), ..self.clone() })
    }

    /// These settings on the computer's default devices: what plays while the chosen ones are
    /// missing (an interface unplugged).
    pub fn on_default_devices(&self) -> Self {
        Self { simulate: self.simulate, ..Self::default() }
    }

    /// Whether `other` chose the same devices.
    pub fn same_devices(&self, other: &Self) -> bool {
        let id = |d: &Option<DeviceChoice>| d.as_ref().map(|d| d.id.clone());
        (self.backend, id(&self.output), id(&self.input))
            == (other.backend, id(&other.output), id(&other.input))
    }
}

/// A sound card just plugged in, offered to the DJ with the settings that use it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CardOffer {
    /// A controller plugged in with no mixer of its own, that Prisme mixes for: its card is
    /// likely this one.
    pub controller: Option<String>,
    /// The card, as its system tells it apart: what the DJ declines.
    pub card: String,
    pub name: String,
    pub settings: AudioSettings,
}

/// A round trip measured from an output to an input.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "Latency"))]
pub struct LatencyDto {
    pub round_trip_ms: f64,
    pub sample_rate: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AudioInfo {
    pub backend: BackendDto,
    pub device: String,
    pub sample_rate: u32,
    pub output_channels: usize,
    pub input_channels: usize,
    /// Decks of the engine.
    pub decks: usize,
    pub simulated: bool,
    /// The sound card chosen in the settings while it is missing and the audio plays on the
    /// default devices meanwhile (empty: the first one a driver finds).
    pub missing: Option<String>,
}

impl AudioInfo {
    /// Its decks and input channels: how it lays the decks on its turntables.
    pub fn layout(&self) -> (usize, usize) {
        (self.decks, self.input_channels)
    }
}

/// The audio running, as the settings show it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "AudioInfo"))]
pub struct AudioInfoDto {
    #[serde(flatten)]
    pub info: AudioInfo,
    /// Turntables can drive the decks: the audio reads their timecode from its inputs, or
    /// simulates it.
    pub turntables: bool,
}

impl From<AudioInfo> for AudioInfoDto {
    fn from(info: AudioInfo) -> Self {
        Self { turntables: info.simulated || info.input_channels >= 2, info }
    }
}

/// The audio as the settings show it.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "AudioState"))]
pub struct AudioStateDto {
    /// The settings the DJ chose: the audio runs on the default devices while one of theirs is
    /// missing.
    pub settings: AudioSettings,
    /// The audio running, if it does.
    pub info: Option<AudioInfoDto>,
    /// The USB card the DJ is asked to let Prisme open, or refused it.
    pub asked: Option<CardAskedDto>,
    /// The default output another application held as the audio started.
    pub busy: Option<String>,
}

/// A USB card the DJ is asked to let Prisme open (Android asks), or refused it.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "CardAsked"))]
pub struct CardAskedDto {
    pub card: String,
    pub refused: bool,
}

impl AudioStateDto {
    pub fn of(state: &AppState) -> Self {
        let settings = state.preferences.read(|p| p.audio.clone());
        let busy = lock(&state.busy_output).clone();
        let info = lock(&state.session).as_ref().map(|s| {
            // a default output held by another application is there, not missing
            let missing = (busy.is_none() && !s.settings.same_devices(&settings))
                .then(|| settings.output.as_ref().map(|d| d.name.clone()).unwrap_or_default());
            AudioInfo { missing, ..s.info.clone() }.into()
        });
        let asked = usb_asks::asked().map(|a| CardAskedDto { card: a.card, refused: a.refused });
        Self { settings, info, asked, busy }
    }
}

/// The channels a latency measurement may join, counted from 1.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "LatencyChannels"))]
pub struct LatencyChannelsDto {
    pub outputs: usize,
    pub inputs: usize,
}

/// How well the audio keeps up.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "AudioHealth"))]
pub struct AudioHealthDto {
    /// Periods processed too late: each one is an audible click.
    pub over_budget: u64,
    pub worst_callback_us: f64,
    pub input_underruns: u64,
    /// Input frames dropped: more had come than the output took.
    pub input_dropped: u64,
    pub device_lost: bool,
    /// The engine failed and could not be restarted: the audio is silent.
    pub engine_failed: bool,
    /// Frames per period, as the driver calls the engine (0 before the first period).
    pub period_frames: u64,
    /// How long a period lasts, once the driver has called the engine.
    pub period_ms: Option<f64>,
}

impl AudioHealthDto {
    /// The health of a stream at `sample_rate`.
    pub fn of(stats: &StreamStats, sample_rate: u32) -> Self {
        let health = Self::from(stats);
        let period_ms = (health.period_frames > 0 && sample_rate > 0)
            .then(|| health.period_frames as f64 / f64::from(sample_rate) * 1000.0);
        Self { period_ms, ..health }
    }
}

impl From<&StreamStats> for AudioHealthDto {
    fn from(s: &StreamStats) -> Self {
        Self {
            over_budget: s.over_budget.load(Ordering::Relaxed),
            worst_callback_us: s.max_callback_ns.load(Ordering::Relaxed) as f64 / 1000.0,
            input_underruns: s.input_underruns.load(Ordering::Relaxed),
            input_dropped: s.input_dropped.load(Ordering::Relaxed),
            device_lost: s.device_lost.load(Ordering::Relaxed),
            engine_failed: s.callback_failed.load(Ordering::Relaxed),
            period_frames: s.frames.load(Ordering::Relaxed) / s.callbacks.load(Ordering::Relaxed).max(1),
            period_ms: None,
        }
    }
}
