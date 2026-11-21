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
    /// on a thread of its own, off the window's thread (see [`connecting`]).
    #[allow(clippy::disallowed_methods)]
    pub(crate) fn open(
        input: MidiInput,
        port: &MidiInputPort,
        name: &str,
        mut receive: impl FnMut(u64, &[u8]) + Send + 'static,
    ) -> AppResult<Self> {
        connecting(|| input.connect(port, name, move |at_us, bytes, _| receive(at_us, bytes), ()))
            .map(|connection| Self(ManuallyDrop::new(connection)))
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
    /// Opens `port` of `output` as `name`, off the window's thread (see [`connecting`]).
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

/// Opens a port by `connect`. Refused on Android's window's thread: the system hands the device
/// over on that thread, and the port would wait for itself, the window frozen, until it fails.
fn connecting<C, E: Display>(connect: impl FnOnce() -> Result<C, E>) -> AppResult<C> {
    #[cfg(target_os = "android")]
    // SAFETY: two system calls that cannot fail; the window's thread is the process's first.
    if unsafe { libc::gettid() == libc::getpid() } {
        return Err(midi_error("a MIDI port is not opened on the window's thread"));
    }
    attached(connect).map_err(midi_error)
}
