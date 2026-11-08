//! Vocals separation: the model, the queue, and the stems a deck plays.

use crate::dto::{StemMixDto, StemsStatusDto};
use crate::error::AppResult;
use crate::library_service::LibraryService;
use crate::state::AppState;
use crate::stems_service::StemsService;
use prisme_engine::Command;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn stems_status(stems: State<'_, StemsService>) -> StemsStatusDto {
    stems.status()
}

/// Starts downloading the separation model (`stems-model` events).
#[tauri::command]
pub fn download_stems_model(stems: State<'_, StemsService>) {
    stems.download_model();
}

/// Queues the tracks for separation (those already separated are skipped); returns how
/// many were added.
#[tauri::command]
pub fn separate_tracks(stems: State<'_, StemsService>, paths: Vec<String>) -> AppResult<usize> {
    stems.enqueue(paths.into_iter().map(PathBuf::from))
}

/// Queues every track of the library not separated yet.
#[tauri::command]
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

/// Mutes the vocals or the instrumental of the track on `deck`, until the next load.
#[tauri::command]
pub fn set_stem_mix(state: State<'_, AppState>, deck: usize, mix: StemMixDto) -> AppResult<()> {
    state.with_track(deck, |t| t.stem_mix = mix.into())?;
    state.send_if_running(Command::SetStemMix { deck, mix: mix.into() })
}
