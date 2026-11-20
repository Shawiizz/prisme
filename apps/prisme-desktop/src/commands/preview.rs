use super::blocking;
use crate::error::AppResult;
use crate::preview::Previews;
use crate::state::AppState;
use prisme_engine::Command;
use std::path::PathBuf;
use tauri::ipc::Response;
use tauri::{AppHandle, Manager, State};

/// Previews a track of the screen's in the headphones, about a third in, in place of the one
/// previewed, or stops it if it is the one previewed (the selected one's is an action:
/// `library.preview`).
#[tauri::command]
pub async fn toggle_preview(app: AppHandle, path: String) -> AppResult<()> {
    blocking(move || app.state::<Previews>().toggle(&app, PathBuf::from(path))).await
}

/// The preview put at `seconds` into its track: where the screen's strip is clicked.
#[tauri::command]
pub fn seek_preview(state: State<'_, AppState>, seconds: f64) -> AppResult<()> {
    state.send(Command::SeekPreview { seconds })
}

/// The waveform of preview `id` (`dto::waveform_bytes`); empty until it is known.
#[tauri::command]
pub fn preview_waveform(previews: State<'_, Previews>, id: u64) -> Response {
    Response::new(previews.waveform(id))
}
