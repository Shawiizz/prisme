//! Keeps the computer and its screen awake while a deck plays, and a moment after: with
//! turntables nobody touches the keyboard or the mouse during a set. And keeps Prisme at full
//! speed where the system would slow it down. Each system has its own way.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(mobile)]
mod mobile;
#[cfg(desktop)]
mod playing;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
use linux::KeepAwake;
#[cfg(target_os = "macos")]
use macos::KeepAwake;
#[cfg(windows)]
use windows::KeepAwake;

#[cfg(target_os = "linux")]
pub use linux::run_at_full_speed;
#[cfg(target_os = "macos")]
pub use macos::run_at_full_speed;
#[cfg(mobile)]
pub use mobile::run_at_full_speed;
#[cfg(desktop)]
pub use playing::while_playing;
#[cfg(windows)]
pub use windows::run_at_full_speed;
