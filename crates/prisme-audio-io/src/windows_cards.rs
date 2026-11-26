//! The physical card an endpoint of Windows' audio belongs to. A card plugged in has a
//! container of its own, the id Windows gives every function of one device (its outputs, its
//! inputs, the MIDI ports of a controller); what is built into the computer shares the
//! computer's container, and is told apart by the adapter its endpoint is wired to.

use windows::core::{GUID, PCWSTR};
use windows::Win32::Devices::Properties::DEVPKEY_Device_ContainerId;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::{IDeviceTopology, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::StructuredStorage::PropVariantToGUID;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    STGM_READ,
};

/// The container of what is built into the computer, the same for all of it.
const COMPUTER: GUID = GUID::from_u128(0x00000000_0000_0000_ffff_ffffffffffff);

/// The card of the endpoint `endpoint` (its MMDevice id): its container, or for what is built
/// into the computer its adapter.
pub(crate) fn card_of(endpoint: &str) -> Option<String> {
    let _com = Com::enter();
    let wide: Vec<u16> = endpoint.encode_utf16().chain(Some(0)).collect();
    // SAFETY: COM is entered on this thread, and the id is a terminated wide string that
    // outlives the call
    let device = unsafe {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        enumerator.GetDevice(PCWSTR(wide.as_ptr())).ok()?
    };
    match container(&device)? {
        COMPUTER => adapter(&device),
        container => Some(format!("{container:?}")),
    }
}

fn container(device: &IMMDevice) -> Option<GUID> {
    // SAFETY: a DEVPROPKEY is laid out as the PROPERTYKEY the store reads, and the GUID is
    // read from the PROPVARIANT the store returns
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let key = &DEVPKEY_Device_ContainerId as *const _ as *const PROPERTYKEY;
        PropVariantToGUID(&store.GetValue(key).ok()?).ok()
    }
}

/// The adapter `device` is wired to, its device's interface without the pin
/// (`\\?\hdaudio#func_01&ven_10ec…#{…}\elineouttopo` is of `\\?\hdaudio#func_01&ven_10ec…`).
fn adapter(device: &IMMDevice) -> Option<String> {
    // SAFETY: the string the connector returns is the caller's, freed once read
    let wired = unsafe {
        let topology: IDeviceTopology = device.Activate(CLSCTX_ALL, None).ok()?;
        let id = topology.GetConnector(0).ok()?.GetDeviceIdConnectedTo().ok()?;
        let text = id.to_string().ok();
        CoTaskMemFree(Some(id.0 as *const _));
        text?
    };
    Some(wired.split_once("#{").map_or(wired.as_str(), |(device, _)| device).to_owned())
}

/// COM entered on this thread for the time it lives, if it was not already.
struct Com(bool);

impl Com {
    fn enter() -> Self {
        // SAFETY: balanced by CoUninitialize in drop when it succeeded; a thread already in
        // another apartment keeps it (RPC_E_CHANGED_MODE), which serves as well
        Self(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: entered by `enter` on this same thread
            unsafe { CoUninitialize() };
        }
    }
}
