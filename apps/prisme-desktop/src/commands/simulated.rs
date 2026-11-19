//! A simulated HID controller, in debug builds (`controller_hid::simulated`): plugged in, fed
//! reports or the values of its fields, its output reports read back. Prisme takes it as it
//! takes a real one, by its ids: a test or the demo plays a controller without one, over CDP
//! (`window.__TAURI_INTERNALS__.invoke('simulate_hid_plug', { device })`).

use crate::controller_hid::simulated;
use crate::controllers::hid_plugged;
use crate::error::{AppError, AppResult};
use prisme_control::hid::HidInfo;
use prisme_control::Control;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A device to simulate: its product's name, ids, interface and usage, whether it numbers its
/// reports (it does when left out), and the length of each of its input reports by id.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulatedHid {
    product: String,
    vendor_id: u16,
    product_id: u16,
    #[serde(default)]
    interface: Option<u8>,
    #[serde(default)]
    usage_page: u16,
    #[serde(default)]
    usage: u16,
    #[serde(default = "numbered")]
    numbered: bool,
    lengths: BTreeMap<u8, usize>,
}

fn numbered() -> bool {
    true
}

/// A simulated device plugged in: its id for the other commands, and its name for the rest.
#[derive(Debug, Serialize)]
pub struct SimulatedDto {
    id: u32,
    name: String,
}

/// A field of the device's reports, and its value.
#[derive(Debug, Deserialize)]
pub struct FieldValue {
    control: Control,
    value: u32,
}

/// Plugs in a simulated device: the watch finds it as it would a real one.
#[tauri::command]
pub fn simulate_hid_plug(device: SimulatedHid) -> AppResult<SimulatedDto> {
    let info = HidInfo {
        vendor_id: device.vendor_id,
        product_id: device.product_id,
        interface: device.interface,
        usage_page: device.usage_page,
        usage: device.usage,
    };
    let id = simulated::plug(&device.product, info, device.numbered, device.lengths);
    let name = simulated::name_of(&hid_plugged(), id).ok_or_else(|| AppError::Hid("not listed".into()))?;
    Ok(SimulatedDto { id, name })
}

#[tauri::command]
pub fn simulate_hid_unplug(id: u32) {
    simulated::unplug(id);
}

/// Feeds the device `report`, as if it sent it.
#[tauri::command]
pub fn simulate_hid_report(id: u32, report: Vec<u8>) -> AppResult<()> {
    simulated::feed(id, report).map_err(AppError::Hid)
}

/// Feeds the device the reports holding `values`, each the last of its id changed by them.
#[tauri::command]
pub fn simulate_hid_set(id: u32, values: Vec<FieldValue>) -> AppResult<()> {
    let fields = values
        .into_iter()
        .map(|v| match v.control {
            Control::Hid(field) => Ok((field, v.value)),
            other => Err(AppError::Hid(format!("{other} is no field of a HID report"))),
        })
        .collect::<AppResult<Vec<_>>>()?;
    simulated::set(id, &fields).map_err(AppError::Hid)
}

/// The output reports written to the device since this was last asked.
#[tauri::command]
pub fn simulated_hid_written(id: u32) -> Vec<Vec<u8>> {
    simulated::written(id)
}
