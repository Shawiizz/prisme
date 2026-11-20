//! Recording the mix (see `recording`).

use crate::actions::general;
use crate::commands::blocking;
use crate::dto::{RecordSourceDto, RecordingDto};
use crate::error::AppResult;
use crate::folders;
use crate::library_service::LibraryService;
use crate::state::AppState;
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

/// Starts recording `source` (see `general::start_recording`); returns the file being written.
#[tauri::command]
pub fn start_recording(
    app: AppHandle,
    state: State<'_, AppState>,
    library: State<'_, Arc<LibraryService>>,
    source: RecordSourceDto,
) -> AppResult<String> {
    general::start_recording(&app, &state, library.db.clone(), source)
}

/// Ends the recording and writes its tracklist next to it, off the main thread: the end of
/// the recording is written first.
#[tauri::command]
pub async fn stop_recording(app: AppHandle) -> AppResult<Option<RecordingDto>> {
    blocking(move || Ok(general::stop_recording(&app.state::<AppState>()))).await
}

/// Shows a recording in the system's file manager.
#[tauri::command]
pub fn show_recording(path: String) -> AppResult<()> {
    Ok(folders::show(Path::new(&path))?)
}
