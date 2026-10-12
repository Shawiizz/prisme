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

/// How the sound card is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Backend {
    /// Windows' shared audio: works with every device, adds latency.
    #[default]
    Wasapi,
    /// The sound card's own low latency driver.
    Asio,
}

impl Backend {
    pub const ALL: [Backend; 2] = [Backend::Wasapi, Backend::Asio];

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
            #[cfg(not(windows))]
            _ => None,
        }
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            Backend::Wasapi => "WASAPI",
            Backend::Asio => "ASIO",
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
        if let (Some(name), Ok(config)) = (device_name(&device), device.default_output_config()) {
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
        if let (Some(name), Ok(config)) = (device_name(&device), device.default_input_config()) {
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

/// Names of the devices Windows offers now, read without loading any driver: they change
/// when an interface is plugged in or switched on.
pub fn system_devices() -> Vec<String> {
    let _drivers = drivers();
    let Ok(host) = Backend::Wasapi.host() else { return Vec::new() };
    host.devices().map(|devices| devices.filter_map(|d| device_name(&d)).collect()).unwrap_or_default()
}

/// A device of `backend` whose name contains `name` (case-insensitive), or the default one.
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
    let wanted_lower = wanted.to_lowercase();
    let matches = |d: &cpal::Device| device_name(d).is_some_and(|n| n.to_lowercase().contains(&wanted_lower));
    let found = match kind {
        DeviceKind::Output => host.output_devices()?.find(matches),
        DeviceKind::Input => host.input_devices()?.find(matches),
    };
    found.ok_or_else(|| AudioError::DeviceNotFound(wanted.to_string()))
}

fn buffer_range(supported: &cpal::SupportedBufferSize) -> Option<(u32, u32)> {
    match *supported {
        cpal::SupportedBufferSize::Range { min, max } => Some((min, max)),
        cpal::SupportedBufferSize::Unknown => None,
    }
}

pub(crate) fn device_name(device: &cpal::Device) -> Option<String> {
    device.description().ok().map(|d| d.name().to_string())
}
