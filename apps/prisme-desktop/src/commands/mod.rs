//! Commands callable from the frontend (`invoke('name', args)`), by domain.

use crate::error::{AppError, AppResult};

#[cfg(any(target_os = "android", test))]
pub mod apk;
pub mod audio;
pub mod auto_dj;
pub mod controllers;
pub mod crates;
pub mod decks;
pub mod diagnostics;
pub mod edits;
pub mod export;
pub mod folders;
pub mod history;
pub mod import;
pub mod library;
pub mod live;
pub mod marks;
pub mod mixer;
pub mod new_version;
pub mod preferences;
pub mod prepare;
pub mod preview;
pub mod recording;
#[cfg(debug_assertions)]
pub mod round_trip;
pub mod sampler;
pub mod screens;
#[cfg(debug_assertions)]
pub mod simulated;
pub mod stems;
pub mod targets;
pub mod tracks;
pub mod transport;
#[cfg(desktop)]
pub mod update;

/// Runs `work` off the commands' threads: a command that waits for the audio to restart or
/// for the disk would hold the window up meanwhile.
pub async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| AppError::Other(e.to_string()))?
}
