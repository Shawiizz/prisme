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
    /// A USB sound card driven by Prisme itself, whole where the system's audio reaches only
    /// some of its channels.
    Usb,
}

impl Default for Backend {
    fn default() -> Self {
        Self::SYSTEM
    }
}

impl Backend {
    pub const ALL: [Backend; 7] = [
        Backend::Wasapi,
        Backend::Asio,
        Backend::CoreAudio,
        Backend::Alsa,
        Backend::Jack,
        Backend::AAudio,
        Backend::Usb,
    ];

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
        Self::ALL
            .into_iter()
            .filter(|b| match b {
                Backend::Usb => crate::usb::host().is_some(),
                b => b.host_id().is_some_and(|id| hosts.contains(&id)),
            })
            .collect()
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
            Backend::Usb => "USB",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Input,
    Output,
}

/// A device as it is kept: the id its system gives it, by which it is found again, and its
/// name, shown, and by which the same model is found when its id is not there: Windows gives a
/// card without a serial number another id on another USB port.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceRef {
    pub id: String,
    pub name: String,
}

/// The sound card a device is a side of, an interface's outputs and inputs being one card: as
/// its system tells it apart, and as it names it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Card {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// What the system calls it: kept to find it again ([`DeviceRef`]).
    pub id: String,
    pub name: String,
    pub card: Card,
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
    if backend == Backend::Usb {
        return Ok(crate::usb::list());
    }
    let host = backend.host()?;
    let default_in = host.default_input_device().as_ref().and_then(device_id);
    let default_out = host.default_output_device().as_ref().and_then(device_id);
    let mut list = Vec::new();
    let sides = [
        (DeviceKind::Output, host.output_devices()?.collect::<Vec<_>>(), default_out),
        (DeviceKind::Input, host.input_devices()?.collect(), default_in),
    ];
    for (kind, devices, default) in sides {
        for device in devices {
            let (Some(id), Some(name)) = (device_id(&device), device_name(&device)) else { continue };
            let Some(config) = first_config(&device, kind) else { continue };
            list.push(DeviceInfo {
                is_default: Some(&id) == default.as_ref(),
                card: card_of(backend, &device, &id, &name),
                id,
                name,
                kind,
                backend,
                channels: config.channels(),
                sample_rate: config.sample_rate(),
                buffer_range: buffer_range(config.buffer_size()),
            });
        }
    }
    Ok(list)
}

/// A device of the system plugged in now, read without loading any driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Present {
    pub device: DeviceRef,
    pub kind: DeviceKind,
    pub card: Card,
}

/// Whether `device` of `backend` (or its default one) is there, as [`find`] finds it: an
/// interface plugged in or switched on.
pub fn is_available(backend: Backend, kind: DeviceKind, device: Option<&DeviceRef>) -> bool {
    let _drivers = drivers();
    if backend == Backend::Usb {
        return crate::usb::find(device).is_some();
    }
    find(backend, kind, device).is_ok()
}

/// The output the system plays on by default now: it changes when a headset connects, say.
pub fn default_output() -> Option<DeviceRef> {
    let _drivers = drivers();
    let host = Backend::SYSTEM.host().ok()?;
    device_ref(&host.default_output_device()?)
}

/// The devices the system offers now, read without loading any driver: they change when an
/// interface is plugged in or switched on.
pub fn system_devices() -> Vec<Present> {
    let _drivers = drivers();
    let Ok(host) = Backend::SYSTEM.host() else { return Vec::new() };
    let sides = [
        (DeviceKind::Output, host.output_devices().map(Iterator::collect).unwrap_or_else(|_| Vec::new())),
        (DeviceKind::Input, host.input_devices().map(Iterator::collect).unwrap_or_else(|_| Vec::new())),
    ];
    let mut present = Vec::new();
    for (kind, devices) in sides {
        for device in devices {
            let (Some(id), Some(name)) = (device_id(&device), device_name(&device)) else { continue };
            let card = card_of(Backend::SYSTEM, &device, &id, &name);
            present.push(Present { device: DeviceRef { id, name }, kind, card });
        }
    }
    present
}

/// The card `device` of `backend` is a side of, its id `id` and its name `name`.
fn card_of(backend: Backend, device: &cpal::Device, id: &str, name: &str) -> Card {
    let named = |card: String| Card { id: card, name: name.to_owned() };
    match backend {
        // the card as Windows tells it apart (see `windows_cards`); named by its interface
        // ("PIONEER DDJ-SB2"), the endpoint by what it is ("Speakers (…)")
        #[cfg(windows)]
        Backend::Wasapi => {
            let endpoint = id.split_once(':').map_or(id, |(_, endpoint)| endpoint);
            let Some(card) = crate::windows_cards::card_of(endpoint) else {
                return named(id.to_owned());
            };
            let interface = device.description().ok().and_then(|d| d.driver().map(str::to_owned));
            Card { id: card, name: interface.unwrap_or_else(|| name.to_owned()) }
        }
        // a card's PCM is "hw:CARD=<card>,DEV=<n>"
        Backend::Alsa => {
            let card = id.split_once("CARD=").map(|(_, rest)| rest.split(',').next().unwrap_or(rest));
            named(card.map_or_else(|| id.to_owned(), |card| format!("alsa:{card}")))
        }
        // an ASIO driver, a Core Audio device, the JACK server, are one for both directions
        _ => {
            let _ = device;
            named(id.to_owned())
        }
    }
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

/// The device of `backend` that `wanted` is (the default one without it): the one of its id,
/// else, its id gone (the card plugged into another USB port), the one of its name, else of its
/// name once the numbers Windows adds are left out (see [`base_device_name`]).
pub(crate) fn find(
    backend: Backend,
    kind: DeviceKind,
    wanted: Option<&DeviceRef>,
) -> Result<cpal::Device, AudioError> {
    let host = backend.host()?;
    let Some(wanted) = wanted else {
        let default = match kind {
            DeviceKind::Output => host.default_output_device().ok_or(AudioError::NoOutputDevice)?,
            DeviceKind::Input => host
                .default_input_device()
                .ok_or_else(|| AudioError::DeviceNotFound("default input".into()))?,
        };
        // WASAPI's "default device" fails to open again once Windows changed the default:
        // open the device it currently is, the watchdog restarts on the next change
        let Some(current) = device_ref(&default) else { return Ok(default) };
        return find(backend, kind, Some(&current)).or(Ok(default));
    };
    let devices = match kind {
        DeviceKind::Output => host.output_devices()?.collect::<Vec<_>>(),
        DeviceKind::Input => host.input_devices()?.collect(),
    };
    let named: Vec<(Option<String>, String)> =
        devices.iter().map(|d| (device_id(d), device_name(d).unwrap_or_default())).collect();
    pick(&named, wanted)
        .map(|at| devices.into_iter().nth(at).expect("found among them"))
        .ok_or_else(|| AudioError::DeviceNotFound(wanted.name.clone()))
}

/// Of `devices` (their ids and names), where `wanted` is: see [`find`].
fn pick(devices: &[(Option<String>, String)], wanted: &DeviceRef) -> Option<usize> {
    let (name, base) = (wanted.name.to_lowercase(), base_device_name(&wanted.name));
    devices
        .iter()
        .position(|(id, _)| id.as_ref() == Some(&wanted.id))
        .or_else(|| devices.iter().position(|(_, n)| !n.is_empty() && n.to_lowercase() == name))
        .or_else(|| devices.iter().position(|(_, n)| !n.is_empty() && base_device_name(n) == base))
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
        // the other side of a card is the device of its name
        let name = device_name(device)?;
        let host = Backend::AAudio.host().ok()?;
        let others = match other {
            DeviceKind::Output => host.output_devices().ok()?.collect::<Vec<_>>(),
            DeviceKind::Input => host.input_devices().ok()?.collect(),
        };
        let device = others.into_iter().find(|d| device_name(d).as_ref() == Some(&name))?;
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

/// The id the system gives `device`, as cpal writes it ("wasapi:{0.0.0.00000000}.{…}").
pub(crate) fn device_id(device: &cpal::Device) -> Option<String> {
    device.id().ok().map(|id| id.to_string())
}

/// The device `device` is, with its id.
pub(crate) fn device_ref(device: &cpal::Device) -> Option<DeviceRef> {
    Some(DeviceRef { id: device_id(device)?, name: device_name(device)? })
}

#[cfg(test)]
mod pick_tests {
    use super::{pick, DeviceRef};

    fn devices() -> Vec<(Option<String>, String)> {
        [("a", "Speakers (Realtek Audio)"), ("b", "Speakers (2- SL3)"), ("c", "Speakers (SL3)")]
            .map(|(id, name)| (Some(id.to_owned()), name.to_owned()))
            .into()
    }

    fn wanted(id: &str, name: &str) -> DeviceRef {
        DeviceRef { id: id.into(), name: name.into() }
    }

    #[test]
    fn a_device_is_found_by_its_id_whatever_its_name() {
        assert_eq!(pick(&devices(), &wanted("b", "Speakers (SL3)")), Some(1));
    }

    #[test]
    fn its_id_gone_the_same_model_is_found_by_its_name_or_the_one_windows_numbered() {
        assert_eq!(pick(&devices(), &wanted("gone", "Speakers (SL3)")), Some(2));
        assert_eq!(pick(&devices(), &wanted("gone", "Speakers (3- Realtek Audio)")), Some(0));
    }

    #[test]
    fn a_part_of_a_name_is_no_device() {
        assert_eq!(pick(&devices(), &wanted("gone", "Speakers")), None);
        assert_eq!(pick(&devices(), &wanted("gone", "SL3")), None);
    }
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
