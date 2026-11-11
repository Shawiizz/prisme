//! Keeps the computer and its screen awake while the audio runs: with turntables nobody
//! touches the keyboard or the mouse during a set.

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Power::{
    PowerClearRequest, PowerCreateRequest, PowerRequestDisplayRequired, PowerRequestSystemRequired,
    PowerSetRequest, POWER_REQUEST_TYPE,
};
use windows_sys::Win32::System::SystemServices::POWER_REQUEST_CONTEXT_VERSION;
use windows_sys::Win32::System::Threading::{
    POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0,
};

const REQUESTS: [POWER_REQUEST_TYPE; 2] = [PowerRequestSystemRequired, PowerRequestDisplayRequired];

/// A power request held until dropped (`powercfg /requests` lists it).
pub struct KeepAwake(HANDLE);

// SAFETY: a power request is a kernel object handle, usable and closable from any thread.
unsafe impl Send for KeepAwake {}

impl KeepAwake {
    /// `None` if Windows refuses the request: the computer may then sleep as usual.
    pub fn new() -> Option<Self> {
        let mut reason: Vec<u16> = "Prisme is playing".encode_utf16().chain([0]).collect();
        let context = REASON_CONTEXT {
            Version: POWER_REQUEST_CONTEXT_VERSION,
            Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
            Reason: REASON_CONTEXT_0 { SimpleReasonString: reason.as_mut_ptr() },
        };
        // SAFETY: the context and its string outlive the call, which copies them.
        let handle = unsafe { PowerCreateRequest(&context) };
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return None;
        }
        for request in REQUESTS {
            // SAFETY: a valid power request handle.
            unsafe { PowerSetRequest(handle, request) };
        }
        Some(Self(handle))
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        // SAFETY: the handle came from PowerCreateRequest and is closed once.
        unsafe {
            for request in REQUESTS {
                PowerClearRequest(self.0, request);
            }
            CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_is_taken_and_released() {
        let awake = KeepAwake::new().expect("Windows grants power requests");
        drop(awake);
        assert!(KeepAwake::new().is_some(), "and again after release");
    }
}
