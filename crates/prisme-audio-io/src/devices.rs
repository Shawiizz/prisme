//! Audio backends and their devices.

use crate::AudioError;
use cpal::traits::{DeviceTrait, HostTrait};
use std::fmt;
use std::sync::{Mutex, MutexGuard};

/// The drivers, ASIO's above all, do not bear being loaded from two threads at once (the
/// process aborts): whatever lists, opens or closes devices holds this.
static DRIVERS: Mutex<()> = Mutex::new(());

pub(crate) fn drivers() -> MutexGuard<'static, ()> {
    DRIVERS.lock().unwrap_or_else(|e| e.into_inner())
}

/// How the sound card is reached. Each system has its own: the one that works with every
/// device ([`Backend::SYSTEM`]), and for some a low latency one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Backend {
    /// Windows' shared audio: works with every device, adds latency.
    Wasapi,
    /// Windows: the sound card's own low latency driver.
    Asio,
    /// The audio of macOS and iOS, low latency as it is.
    CoreAudio,
    /// Linux: the sound card itself, or the sound server through its `default` device.
    Alsa,
    /// Linux: the JACK server (or PipeWire in its place), low latency and shared.
    Jack,
    /// Android's audio: the phone itself, or the sound card plugged into it.
    AAudio,
}

impl Default for Backend {
    fn default() -> Self {
        Self::SYSTEM
    }
}

impl Backend {
    pub const ALL: [Backend; 6] =
        [Backend::Wasapi, Backend::Asio, Backend::CoreAudio, Backend::Alsa, Backend::Jack, Backend::AAudio];

    /// The backend of this system that works with every device.
    #[cfg(windows)]
    pub const SYSTEM: Backend = Backend::Wasapi;
    #[cfg(target_vendor = "apple")]
    pub const SYSTEM: Backend = Backend::CoreAudio;
    #[cfg(target_os = "android")]
    pub const SYSTEM: Backend = Backend::AAudio;
    #[cfg(not(any(windows, target_vendor = "apple", target_os = "android")))]
    pub const SYSTEM: Backend = Backend::Alsa;

    /// Backends this computer offers.
    pub fn available() -> Vec<Backend> {
        let hosts = cpal::available_hosts();
        Self::ALL.into_iter().filter(|b| b.host_id().is_some_and(|id| hosts.contains(&id))).collect()
    }

    pub(crate) fn host(self) -> Result<cpal::Host, AudioError> {
        let id = self.host_id().ok_or(AudioError::BackendUnavailable(self))?;
        cpal::host_from_id(id).map_err(|_| AudioError::BackendUnavailable(self))
    }

    fn host_id(self) -> Option<cpal::HostId> {
        match self {
            #[cfg(windows)]
            Backend::Wasapi => Some(cpal::HostId::Wasapi),
            #[cfg(windows)]
            Backend::Asio => Some(cpal::HostId::Asio),
            #[cfg(target_vendor = "apple")]
            Backend::CoreAudio => Some(cpal::HostId::CoreAudio),
            #[cfg(target_os = "android")]
            Backend::AAudio => Some(cpal::HostId::AAudio),
            #[cfg(target_os = "linux")]
            Backend::Alsa => Some(cpal::HostId::Alsa),
            #[cfg(target_os = "linux")]
            Backend::Jack => Some(cpal::HostId::Jack),
            // the backends of the other systems
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            Backend::Wasapi => "WASAPI",
            Backend::Asio => "ASIO",
            Backend::CoreAudio => "Core Audio",
            Backend::Alsa => "ALSA",
            Backend::Jack => "JACK",
            Backend::AAudio => "AAudio",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Input,
    Output,
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub kind: DeviceKind,
    pub backend: Backend,
    pub channels: u16,
    pub sample_rate: u32,
    /// Periods the driver accepts (frames), when it says.
    pub buffer_range: Option<(u32, u32)>,
    pub is_default: bool,
}

/// Input and output devices of `backend` (an ASIO driver appears once in each list).
pub fn list_devices(backend: Backend) -> Result<Vec<DeviceInfo>, AudioError> {
    let _drivers = drivers();
    let host = backend.host()?;
    let default_in = host.default_input_device().as_ref().and_then(device_name);
    let default_out = host.default_output_device().as_ref().and_then(device_name);
    let mut list = Vec::new();
    for device in host.output_devices()? {
        if let (Some(name), Some(config)) = (device_name(&device), first_config(&device, DeviceKind::Output))
        {
            list.push(DeviceInfo {
                is_default: Some(&name) == default_out.as_ref(),
                name,
                kind: DeviceKind::Output,
                backend,
                channels: config.channels(),
                sample_rate: config.sample_rate(),
                buffer_range: buffer_range(config.buffer_size()),
            });
        }
    }
    for device in host.input_devices()? {
        if let (Some(name), Some(config)) = (device_name(&device), first_config(&device, DeviceKind::Input)) {
            list.push(DeviceInfo {
                is_default: Some(&name) == default_in.as_ref(),
                name,
                kind: DeviceKind::Input,
                backend,
                channels: config.channels(),
                sample_rate: config.sample_rate(),
                buffer_range: buffer_range(config.buffer_size()),
            });
        }
    }
    Ok(list)
}

/// Whether a device of `backend` whose name contains `name` (or the default one) is
/// there: an interface plugged in or switched on.
pub fn is_available(backend: Backend, kind: DeviceKind, name: Option<&str>) -> bool {
    let _drivers = drivers();
    find(backend, kind, name).is_ok()
}

/// The output the system plays on by default now: it changes when a headset connects, say.
pub fn default_output_name() -> Option<String> {
    let _drivers = drivers();
    let host = Backend::SYSTEM.host().ok()?;
    device_name(&host.default_output_device()?)
}

/// Names of the devices the system offers now, read without loading any driver: they change
/// when an interface is plugged in or switched on.
pub fn system_devices() -> Vec<String> {
    let _drivers = drivers();
    let Ok(host) = Backend::SYSTEM.host() else { return Vec::new() };
    host.devices().map(|devices| devices.filter_map(|d| device_name(&d)).collect()).unwrap_or_default()
}

/// A device's name as Windows gives any device of its model, lower case: without the
/// "2- " it puts in for a second one, or for one plugged into another port
/// (`Line (2- SL3)`). Two names for the same interface, wherever it is plugged.
pub fn base_device_name(name: &str) -> String {
    let lower = name.to_lowercase();
    let mut base = String::with_capacity(lower.len());
    let mut rest = lower.as_str();
    while let Some(start) = rest.find(|c: char| c.is_ascii_digit()) {
        let end = rest[start..].find(|c: char| !c.is_ascii_digit()).map_or(rest.len(), |n| start + n);
        let numbered = (start == 0 || rest[..start].ends_with('(')) && rest[end..].starts_with("- ");
        base.push_str(&rest[..start]);
        if !numbered {
            base.push_str(&rest[start..end]);
        }
        rest = &rest[if numbered { end + 2 } else { end }..];
    }
    base.push_str(rest);
    base
}

/// The device of `backend` named `name`, else one whose name contains it, else the same
/// once the numbers Windows adds are left out (see [`base_device_name`]); all
/// case-insensitive. The default one without a name.
pub(crate) fn find(
    backend: Backend,
    kind: DeviceKind,
    name: Option<&str>,
) -> Result<cpal::Device, AudioError> {
    let host = backend.host()?;
    let Some(wanted) = name else {
        let default = match kind {
            DeviceKind::Output => host.default_output_device().ok_or(AudioError::NoOutputDevice)?,
            DeviceKind::Input => host
                .default_input_device()
                .ok_or_else(|| AudioError::DeviceNotFound("default input".into()))?,
        };
        // WASAPI's "default device" fails to open again once Windows changed the default:
        // open the device it currently is, the watchdog restarts on the next change
        let Some(name) = device_name(&default) else { return Ok(default) };
        return find(backend, kind, Some(&name)).or(Ok(default));
    };
    let devices: Vec<(cpal::Device, String)> = match kind {
        DeviceKind::Output => host.output_devices()?.collect::<Vec<_>>(),
        DeviceKind::Input => host.input_devices()?.collect(),
    }
    .into_iter()
    .filter_map(|d| device_name(&d).map(|n| (d, n)))
    .collect();
    let (wanted_lower, wanted_base) = (wanted.to_lowercase(), base_device_name(wanted));
    let found = devices
        .iter()
        .position(|(_, n)| n.to_lowercase() == wanted_lower)
        .or_else(|| devices.iter().position(|(_, n)| n.to_lowercase().contains(&wanted_lower)))
        .or_else(|| devices.iter().position(|(_, n)| base_device_name(n).contains(&wanted_base)));
    found
        .map(|i| devices.into_iter().nth(i).expect("found among them").0)
        .ok_or_else(|| AudioError::DeviceNotFound(wanted.to_string()))
}

fn buffer_range(supported: &cpal::SupportedBufferSize) -> Option<(u32, u32)> {
    match *supported {
        cpal::SupportedBufferSize::Range { min, max } => Some((min, max)),
        cpal::SupportedBufferSize::Unknown => None,
    }
}

/// The configurations to open `device` with, the system's default last. A sound card plugged
/// into a phone comes first with all its channels: Android offers it in stereo, which would
/// leave out the outputs past the master and the second turntable's inputs.
pub(crate) fn configs(
    device: &cpal::Device,
    kind: DeviceKind,
) -> Result<Vec<cpal::SupportedStreamConfig>, AudioError> {
    let default = match kind {
        DeviceKind::Output => device.default_output_config()?,
        DeviceKind::Input => device.default_input_config()?,
    };
    #[cfg(target_os = "android")]
    if let Some(widest) = widest(device, kind, &default) {
        return Ok(vec![widest, default]);
    }
    Ok(vec![default])
}

/// The configuration `device` is first tried with.
fn first_config(device: &cpal::Device, kind: DeviceKind) -> Option<cpal::SupportedStreamConfig> {
    configs(device, kind).ok()?.into_iter().next()
}

/// The configuration of a USB sound card with all its channels, when it has more than
/// `default`, at its rate and in its format.
#[cfg(target_os = "android")]
fn widest(
    device: &cpal::Device,
    kind: DeviceKind,
    default: &cpal::SupportedStreamConfig,
) -> Option<cpal::SupportedStreamConfig> {
    if device.description().ok()?.interface_type() != cpal::InterfaceType::Usb {
        return None;
    }
    let ranges = supported(device, kind)?;
    let own: Vec<u16> = ranges.iter().map(|r| r.channels()).collect();
    let other = || {
        let other = match kind {
            DeviceKind::Output => DeviceKind::Input,
            DeviceKind::Input => DeviceKind::Output,
        };
        let device = find(Backend::AAudio, other, Some(&device_name(device)?)).ok()?;
        Some(supported(&device, other)?.iter().map(|r| r.channels()).collect::<Vec<_>>())
    };
    let channels = card_channels(&own, other).filter(|&c| c > default.channels())?;
    ranges
        .into_iter()
        .filter(|r| r.sample_format() == default.sample_format() && r.channels() == channels)
        .find_map(|r| r.try_with_sample_rate(default.sample_rate()))
}

#[cfg(target_os = "android")]
fn supported(device: &cpal::Device, kind: DeviceKind) -> Option<Vec<cpal::SupportedStreamConfigRange>> {
    match kind {
        DeviceKind::Output => device.supported_output_configs().ok().map(Iterator::collect),
        DeviceKind::Input => device.supported_input_configs().ok().map(Iterator::collect),
    }
}

/// The channel counts the AAudio host of cpal offers for a side of a device Android
/// describes nothing of: guesses, not the device's.
#[cfg(any(target_os = "android", test))]
const GUESSED_CHANNELS: [u16; 5] = [1, 2, 4, 6, 8];

/// The channels of a side of a sound card that offers `own` channel counts: the most of
/// them, or, when they are cpal's guesses, the most of the `other` side's. Android describes
/// no input of a UMC404HD, and opened in eight channels its four arrive mixed up.
#[cfg(any(target_os = "android", test))]
fn card_channels(own: &[u16], other: impl FnOnce() -> Option<Vec<u16>>) -> Option<u16> {
    let known = |counts: &[u16]| {
        let mut set = counts.to_vec();
        set.sort_unstable();
        set.dedup();
        (set != GUESSED_CHANNELS).then(|| set.last().copied()).flatten()
    };
    known(own).or_else(|| known(&other()?))
}

pub(crate) fn device_name(device: &cpal::Device) -> Option<String> {
    device.description().ok().map(|d| d.name().to_string())
}

#[cfg(test)]
mod card_channels_tests {
    use super::{card_channels, GUESSED_CHANNELS};

    #[test]
    fn a_side_described_has_its_most_channels() {
        assert_eq!(card_channels(&[2, 4, 2, 4], || panic!("the other side is not asked")), Some(4));
    }

    #[test]
    fn a_side_described_by_guesses_has_the_channels_of_the_other() {
        let guessed: Vec<u16> = GUESSED_CHANNELS.iter().copied().cycle().take(15).collect();
        assert_eq!(card_channels(&guessed, || Some(vec![2, 3, 4])), Some(4));
        assert_eq!(card_channels(&guessed, || Some(guessed.clone())), None);
        assert_eq!(card_channels(&guessed, || None), None);
    }
}

#[cfg(test)]
mod base_name_tests {
    use super::base_device_name;

    #[test]
    fn windows_numbers_are_left_out() {
        assert_eq!(base_device_name("Line (2- SL3)"), "line (sl3)");
        assert_eq!(base_device_name("2- DJControl Inpulse 200"), "djcontrol inpulse 200");
        assert_eq!(base_device_name("Speakers (12- USB Audio)"), "speakers (usb audio)");
    }

    #[test]
    fn numbers_of_the_name_stay() {
        assert_eq!(base_device_name("UMC404HD 192k"), "umc404hd 192k");
        assert_eq!(base_device_name("Out 3-4 (UMC404HD)"), "out 3-4 (umc404hd)");
        assert_eq!(base_device_name("Line (SL3)"), "line (sl3)");
    }
}
