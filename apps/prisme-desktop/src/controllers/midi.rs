//! The MIDI ports Prisme opens, input and output: opened here only (`clippy.toml` keeps
//! midir's `connect` out of the rest of the application) and closed as they are dropped. On
//! Android a connection dropped without being closed keeps its device open, a sound card the
//! USB driver would play on with it, and an input's thread reading on; and the system aborts
//! the application when a thread unknown to the Java virtual machine lets go of a device, which
//! opening does too when it fails.

use super::midi_error;
use crate::error::AppResult;
use midir::{
    MidiInput, MidiInputConnection, MidiInputPort, MidiOutput, MidiOutputConnection, MidiOutputPort,
    SendError,
};
use std::fmt::Display;
use std::mem::ManuallyDrop;
use std::panic::{catch_unwind, AssertUnwindSafe};
use tracing::error;

#[cfg(target_os = "android")]
use crate::android_jni::attached;

#[cfg(not(target_os = "android"))]
fn attached<T>(f: impl FnOnce() -> T) -> T {
    f()
}

/// A MIDI input open, closed once dropped.
pub(crate) struct Input(ManuallyDrop<MidiInputConnection<()>>);

impl Input {
    /// Opens `port` of `input` as `name`, each message given to `receive` with its time in µs
    /// on a thread of its own. May take seconds (see [`connecting`]).
    #[allow(clippy::disallowed_methods)]
    pub(crate) fn open(
        input: MidiInput,
        port: &MidiInputPort,
        name: &str,
        receive: impl FnMut(u64, &[u8]) + Send + 'static,
    ) -> AppResult<Self> {
        let mut receive = surviving(receive);
        connecting(|| input.connect(port, name, move |at_us, bytes, _| receive(at_us, bytes), ()))
            .map(|connection| Self(ManuallyDrop::new(connection)))
    }
}

/// `receive`, a message that makes it panic logged and dropped: the panic would end the
/// system's reading thread or the application (a callback of the system's), and midir's close
/// would panic again on Android, the reading thread found ended.
fn surviving(mut receive: impl FnMut(u64, &[u8])) -> impl FnMut(u64, &[u8]) {
    move |at_us, bytes| {
        if catch_unwind(AssertUnwindSafe(|| receive(at_us, bytes))).is_err() {
            error!("MIDI message {bytes:02x?} failed");
        }
    }
}

impl Drop for Input {
    fn drop(&mut self) {
        // SAFETY: taken once, as it is dropped, and never used again.
        let connection = unsafe { ManuallyDrop::take(&mut self.0) };
        attached(|| {
            connection.close();
        });
    }
}

/// A MIDI output open, closed once dropped.
pub(crate) struct Output(ManuallyDrop<MidiOutputConnection>);

impl Output {
    /// Opens `port` of `output` as `name`. May take seconds (see [`connecting`]).
    #[allow(clippy::disallowed_methods)]
    pub(crate) fn open(output: MidiOutput, port: &MidiOutputPort, name: &str) -> AppResult<Self> {
        connecting(|| output.connect(port, name)).map(|connection| Self(ManuallyDrop::new(connection)))
    }

    pub(crate) fn send(&mut self, message: &[u8]) -> Result<(), SendError> {
        self.0.send(message)
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        // SAFETY: taken once, as it is dropped, and never used again.
        let connection = unsafe { ManuallyDrop::take(&mut self.0) };
        attached(|| {
            connection.close();
        });
    }
}

/// Opens a port by `connect`. On Android the system hands the device over once it is ready,
/// midir waiting up to 5 s for it: never on a page's call or during the start.
fn connecting<C, E: Display>(connect: impl FnOnce() -> Result<C, E>) -> AppResult<C> {
    attached(connect).map_err(midi_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_that_fails_is_dropped_and_the_next_taken() {
        let mut taken = Vec::new();
        let mut receive = surviving(|at_us, bytes: &[u8]| {
            assert_ne!(bytes, [0x90, 0x3c, 0x7f], "a bug");
            taken.push(at_us);
        });
        receive(1, &[0x90, 0x3c, 0x7f]);
        receive(2, &[0x80, 0x3c, 0x00]);
        drop(receive);
        assert_eq!(taken, [2]);
    }
}
