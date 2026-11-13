//! Keeps the computer and its screen awake while the audio runs: with turntables nobody
//! touches the keyboard or the mouse during a set. And keeps Prisme at full speed where the
//! system would slow it down. Each system has its own way.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(mobile)]
mod mobile;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::{run_at_full_speed, KeepAwake};
#[cfg(target_os = "macos")]
pub use macos::{run_at_full_speed, KeepAwake};
#[cfg(mobile)]
pub use mobile::{run_at_full_speed, KeepAwake};
#[cfg(windows)]
pub use windows::{run_at_full_speed, KeepAwake};
