use crate::dto::{waveform_bytes, TrackInfoDto};
use crate::error::{AppError, AppResult};
use crate::library_service::LibraryService;
use crate::state::{lock, AppState};
use crate::track_loading;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::ipc::Response;
use tauri::{AppHandle, State};

/// Puts a track on a deck as soon as it can play; its waveform and analysis follow if
/// they were not known (see `track_loading`).
#[tauri::command]
pub async fn load_track(
    app: AppHandle,
    state: State<'_, AppState>,
    library: State<'_, Arc<LibraryService>>,
    deck: usize,
    path: String,
) -> AppResult<TrackInfoDto> {
    AppState::check_deck(deck)?;
    let file = PathBuf::from(&path);
    let library = library.inner().clone();
    let (track, completion) =
        tauri::async_runtime::spawn_blocking(move || track_loading::prepare(file, &library))
            .await
            .map_err(|e| AppError::Other(e.to_string()))??;
    let info = track.info(deck);
    let commands = track.load_commands(deck);
    let load_id = track.load_id;
    // kept for audio restarts
    lock(&state.tracks)[deck] = Some(track);
    for c in commands {
        state.send_if_running(c)?;
    }
    tauri::async_runtime::spawn_blocking(move || completion.run(&app, deck, load_id));
    Ok(info)
}

/// The waveform of the track on `deck`, in binary (`dto::waveform_bytes`); empty when
/// it is not known yet or another track was loaded meanwhile.
#[tauri::command]
pub fn deck_waveform(state: State<'_, AppState>, deck: usize, load_id: u64) -> Response {
    let bytes = state.with_loaded(deck, load_id, |t| t.waveform.as_ref().map(waveform_bytes)).flatten();
    Response::new(bytes.unwrap_or_default())
}

/// Tracks currently loaded on the decks (to rebuild the UI after a reload).
#[tauri::command]
pub fn loaded_tracks(state: State<'_, AppState>) -> Vec<Option<TrackInfoDto>> {
    lock(&state.tracks).iter().enumerate().map(|(deck, t)| t.as_ref().map(|t| t.info(deck))).collect()
}
