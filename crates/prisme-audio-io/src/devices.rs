//! Listing and finding sound devices.

use crate::AudioError;
use cpal::traits::{DeviceTrait, HostTrait};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Input,
    Output,
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub kind: DeviceKind,
    pub host: String,
    pub channels: u16,
    pub sample_rate: u32,
    pub is_default: bool,
}

/// Lists the input and output devices of the default host.
pub fn list_devices() -> Result<Vec<DeviceInfo>, AudioError> {
    let host = cpal::default_host();
    let host_name = format!("{:?}", host.id());
    let default_in = host.default_input_device().and_then(|d| d.name().ok());
    let default_out = host.default_output_device().and_then(|d| d.name().ok());
    let mut list = Vec::new();
    for device in host.output_devices()? {
        if let Ok(config) = device.default_output_config() {
            let name = device.name()?;
            list.push(DeviceInfo {
                is_default: Some(&name) == default_out.as_ref(),
                name,
                kind: DeviceKind::Output,
                host: host_name.clone(),
                channels: config.channels(),
                sample_rate: config.sample_rate().0,
            });
        }
    }
    for device in host.input_devices()? {
        if let Ok(config) = device.default_input_config() {
            let name = device.name()?;
            list.push(DeviceInfo {
                is_default: Some(&name) == default_in.as_ref(),
                name,
                kind: DeviceKind::Input,
                host: host_name.clone(),
                channels: config.channels(),
                sample_rate: config.sample_rate().0,
            });
        }
    }
    Ok(list)
}

/// Finds a device whose name contains `name` (case-insensitive), or the default one.
pub(crate) fn find(kind: DeviceKind, name: Option<&str>) -> Result<cpal::Device, AudioError> {
    let host = cpal::default_host();
    let Some(wanted) = name else {
        return match kind {
            DeviceKind::Output => host.default_output_device().ok_or(AudioError::NoOutputDevice),
            DeviceKind::Input => host.default_input_device().ok_or(AudioError::DeviceNotFound("default input".into())),
        };
    };
    let wanted_lower = wanted.to_lowercase();
    let mut devices: Box<dyn Iterator<Item = cpal::Device>> = match kind {
        DeviceKind::Output => Box::new(host.output_devices()?),
        DeviceKind::Input => Box::new(host.input_devices()?),
    };
    devices
        .find(|d| d.name().map(|n| n.to_lowercase().contains(&wanted_lower)).unwrap_or(false))
        .ok_or_else(|| AudioError::DeviceNotFound(wanted.to_string()))
}
