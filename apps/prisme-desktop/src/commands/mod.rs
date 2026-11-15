//! Commands callable from the frontend (`invoke('name', args)`), by domain.

use crate::error::{AppError, AppResult};

pub mod audio;
pub mod controllers;
pub mod crates;
pub mod cues;
pub mod decks;
pub mod diagnostics;
pub mod edits;
pub mod folders;
pub mod fx;
pub mod history;
pub mod import;
pub mod library;
pub mod marks;
pub mod mixer;
pub mod preferences;
pub mod prepare;
pub mod preview;
pub mod recording;
pub mod sampler;
pub mod screens;
pub mod stems;
pub mod tracks;
pub mod transport;

/// Runs `work` off the commands' threads: a command that waits for the audio to restart or
/// for the disk would hold the window up meanwhile.
pub async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| AppError::Other(e.to_string()))?
}
