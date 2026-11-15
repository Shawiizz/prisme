use super::blocking;
use crate::dto::PreviewDto;
use crate::error::AppResult;
use crate::preview::Previews;
use crate::state::AppState;
use prisme_engine::Command;
use std::path::PathBuf;
use tauri::ipc::Response;
use tauri::{AppHandle, Manager, State};

/// Previews a track in the headphones, about a third in, in place of the one previewed;
/// `None` if another was asked for meanwhile.
#[tauri::command]
pub async fn preview_track(app: AppHandle, path: String) -> AppResult<Option<PreviewDto>> {
    blocking(move || app.state::<Previews>().start(&app, PathBuf::from(path))).await
}

#[tauri::command]
pub fn stop_preview(state: State<'_, AppState>, previews: State<'_, Previews>) -> AppResult<()> {
    previews.stop(&state)
}

#[tauri::command]
pub fn seek_preview(state: State<'_, AppState>, seconds: f64) -> AppResult<()> {
    state.send(Command::SeekPreview { seconds })
}

/// Moves the preview by `seconds`, forwards or back.
#[tauri::command]
pub fn skip_preview(state: State<'_, AppState>, seconds: f64) -> AppResult<()> {
    state.send(Command::SkipPreview { seconds })
}

/// The waveform of preview `id` (`dto::waveform_bytes`); empty until it is known.
#[tauri::command]
pub fn preview_waveform(previews: State<'_, Previews>, id: u64) -> Response {
    Response::new(previews.waveform(id))
}
