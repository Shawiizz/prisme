//! Files dropped from the file manager. The page's own tracks drag onto the decks and the
//! crates in the page; on Windows and macOS the window's drag and drop would take those drags
//! from it, so it is off there (`dragDropEnabled: false`) and the paths of the files dropped are
//! read here, each system its own way. On Linux the window tells them and lets the page's own
//! drags through.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "macos")]
pub use macos::listen;
#[cfg(windows)]
pub use windows::listen;
