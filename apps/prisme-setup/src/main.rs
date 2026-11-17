//! Prisme's installer for Windows: a window of Prisme's own while the NSIS installer it carries
//! runs without one, then Prisme opens. Nothing on the other systems, which install it their way.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod setup;

#[cfg(windows)]
fn main() {
    setup::run();
}

#[cfg(not(windows))]
fn main() {}
