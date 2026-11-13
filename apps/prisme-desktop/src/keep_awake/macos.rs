//! macOS: a power assertion (`pmset -g assertions` lists it).

use std::ffi::{c_char, c_void, CString};

type CfString = *const c_void;

const UTF8: u32 = 0x0800_0100;
const ASSERTION_ON: u32 = 255;
const SUCCESS: i32 = 0;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithCString(allocator: *const c_void, text: *const c_char, encoding: u32) -> CfString;
    fn CFRelease(object: *const c_void);
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOPMAssertionCreateWithName(kind: CfString, level: u32, name: CfString, id: *mut u32) -> i32;
    fn IOPMAssertionRelease(id: u32) -> i32;
}

/// An assertion against the sleep of the display (and so of the computer), held until
/// dropped.
pub struct KeepAwake(u32);

impl KeepAwake {
    /// `None` if macOS refuses the assertion: the computer may then sleep as usual.
    pub fn new() -> Option<Self> {
        let text = |s: &str| {
            let text = CString::new(s).ok()?;
            // SAFETY: a NUL-terminated string, copied by the call.
            let string = unsafe { CFStringCreateWithCString(std::ptr::null(), text.as_ptr(), UTF8) };
            (!string.is_null()).then_some(string)
        };
        let (kind, name) = (text("PreventUserIdleDisplaySleep")?, text("Prisme is playing")?);
        let mut id = 0;
        // SAFETY: two valid strings, released below, and a place for the identifier.
        let created = unsafe {
            let created = IOPMAssertionCreateWithName(kind, ASSERTION_ON, name, &mut id);
            CFRelease(kind);
            CFRelease(name);
            created
        };
        (created == SUCCESS).then_some(Self(id))
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        // SAFETY: the identifier came from IOPMAssertionCreateWithName and is released once.
        unsafe { IOPMAssertionRelease(self.0) };
    }
}

/// macOS does not put to sleep (App Nap) an application that plays audio: nothing to opt
/// out of.
pub fn run_at_full_speed() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_assertion_is_taken_and_released() {
        let awake = KeepAwake::new().expect("macOS grants power assertions");
        drop(awake);
        assert!(KeepAwake::new().is_some(), "and again after release");
    }
}
