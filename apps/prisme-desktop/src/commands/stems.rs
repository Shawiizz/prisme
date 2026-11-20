//! Stem separation: the models, the queue, and the stems a deck plays.

use crate::error::AppResult;
use crate::library_service::LibraryService;
use crate::stems_service::StemsService;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

/// Starts downloading the separation model `id`.
#[tauri::command]
pub fn download_stems_model(stems: State<'_, StemsService>, id: String) -> AppResult<()> {
    stems.download_model(&id)
}

/// Separates with the small models (`fast`) or the large ones from now on, and downloads those
/// missing: the tracks they were offered for follow once they are in.
#[tauri::command]
pub fn download_stems_models(stems: State<'_, StemsService>, fast: bool) -> AppResult<()> {
    stems.download_for(fast)
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
