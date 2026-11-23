//! Prisme installing and updating itself, as the programs of today do: a new version is
//! downloaded in the background, checked against the releases' key, and takes the place of the
//! one running, which goes on until Prisme quits. The next launch is the new version, with no
//! installer to go through. Android and the system's packages (.deb) install their own way.

mod error;
mod identity;
mod install;
mod manifest;
mod package;
mod signature;
mod version;
#[cfg(windows)]
pub mod windows;

pub use error::Error;
pub use identity::{Identity, APP_ID, EXE, PUBLISHER};
pub use install::{wait_for_exit, Install};
pub use manifest::{manifest_url, Manifest, Package};
pub use package::unpack;
pub use signature::verify;
pub use version::newer;

use std::time::Duration;

/// How long a new version waits for the one before it to quit.
const QUITTING: Duration = Duration::from_secs(15);

/// What Prisme's arguments ask before anything starts: on Windows its uninstallation, the exit
/// code to end with; after an update, to wait for the version before it to quit
/// (`--after <pid>`), two sharing the library.
pub fn on_launch(identity: &Identity) -> Option<i32> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(windows)]
    if let Some(code) = windows::on_arguments(identity, &arguments) {
        return Some(code);
    }
    let _ = identity;
    if let [flag, pid] = arguments.as_slice() {
        if let (true, Ok(pid)) = (flag == "--after", pid.parse()) {
            wait_for_exit(pid, QUITTING);
        }
    }
    None
}
