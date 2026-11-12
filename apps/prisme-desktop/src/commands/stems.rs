//! Stem separation: the models, the queue, and the stems a deck plays.

use crate::dto::{StemMixDto, StemsStatusDto};
use crate::error::AppResult;
use crate::library_service::LibraryService;
use crate::state::AppState;
use crate::stems_service::{self, StemsService};
use prisme_engine::Command;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn stems_status(stems: State<'_, StemsService>) -> StemsStatusDto {
    stems.status()
}

/// Whether a graphics card can separate (looked for once, off the window's thread).
#[tauri::command(async)]
pub fn stems_gpu() -> bool {
    stems_service::has_gpu()
}

/// Starts downloading the separation model `id` (`stems-model` events).
#[tauri::command]
pub fn download_stems_model(stems: State<'_, StemsService>, id: String) -> AppResult<()> {
    stems.download_model(&id)
}

/// Queues the tracks for separation (those the installed models have nothing to add to are
/// skipped); returns how many were added.
#[tauri::command]
pub fn separate_tracks(stems: State<'_, StemsService>, paths: Vec<String>) -> AppResult<usize> {
    stems.enqueue(paths.into_iter().map(PathBuf::from))
}

/// Queues every track of the library not fully separated yet (off the main thread: a large
/// library takes a while to look over).
#[tauri::command(async)]
pub fn separate_library(
    stems: State<'_, StemsService>,
    library: State<'_, Arc<LibraryService>>,
) -> AppResult<usize> {
    let mut paths: Vec<PathBuf> = library.db.versions()?.into_keys().map(PathBuf::from).collect();
    paths.sort();
    stems.enqueue(paths)
}

#[tauri::command]
pub fn cancel_separation(stems: State<'_, StemsService>) {
    stems.cancel();
}

/// Sets how the track on `deck` plays each part, until the next load: the parts it turns on
/// or off from the next bar if `on_bar` (see `Command::SetStemMix`).
#[tauri::command]
pub fn set_stem_mix(state: State<'_, AppState>, deck: usize, mix: StemMixDto, on_bar: bool) -> AppResult<()> {
    state.with_track(deck, |t| t.stem_mix = mix)?;
    state.send_if_running(Command::SetStemMix { deck, mix: mix.into(), on_bar })
}
