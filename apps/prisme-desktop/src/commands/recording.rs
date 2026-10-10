//! Recording the mix (see `recording`).

use crate::dto::{RecordSourceDto, RecordingDto};
use crate::error::{AppError, AppResult};
use crate::library_service::LibraryService;
use crate::recording::Recorder;
use crate::state::{lock, AppState};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

/// Where recordings go, in the documents: out of the music folder, so that they do not
/// end up in the library.
const FOLDER: &str = "Prisme Recordings";

/// Starts recording; returns the file being written.
#[tauri::command]
pub fn start_recording(
    app: AppHandle,
    state: State<'_, AppState>,
    source: RecordSourceDto,
) -> AppResult<String> {
    let mut recorder = lock(&state.recorder);
    if recorder.is_some() {
        return Err(AppError::Other("already recording".into()));
    }
    let documents = app.path().document_dir().map_err(|e| AppError::Other(e.to_string()))?;
    let started = Recorder::start(&state, source.into(), &documents.join(FOLDER))?;
    let path = started.path().display().to_string();
    *recorder = Some(started);
    Ok(path)
}

/// Ends the recording and writes its tracklist next to it.
#[tauri::command]
pub fn stop_recording(
    state: State<'_, AppState>,
    library: State<'_, Arc<LibraryService>>,
) -> AppResult<Option<RecordingDto>> {
    let Some(recorder) = lock(&state.recorder).take() else { return Ok(None) };
    let summary = recorder.stop(&state, &library.db)?;
    Ok(Some(RecordingDto {
        path: summary.path.display().to_string(),
        seconds: summary.seconds,
        tracks: summary.tracks,
    }))
}
