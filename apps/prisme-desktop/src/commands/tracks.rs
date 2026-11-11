use crate::dto::{waveform_bytes, SeparationDto, TrackInfoDto};
use crate::error::{AppError, AppResult};
use crate::library_service::LibraryService;
use crate::state::{lock, AppState};
use crate::track_loading;
use prisme_engine::{Command, Stem};
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
    let commands = track.load_commands(deck, state.preferences.read(|p| p.dj.auto_gain));
    // kept for audio restarts
    lock(&state.tracks)[deck] = Some(track);
    for c in commands {
        state.send_if_running(c)?;
    }
    tauri::async_runtime::spawn_blocking(move || completion.run(&app));
    Ok(info)
}

/// Puts the track of deck `from` on `deck` where it is, playing if it plays (an instant
/// double): nothing is read again.
#[tauri::command]
pub fn double_deck(state: State<'_, AppState>, from: usize, deck: usize) -> AppResult<TrackInfoDto> {
    AppState::check_deck(deck)?;
    let track = state.with_track(from, |t| t.double())?;
    let info = track.info(deck);
    let commands = track.load_commands(deck, state.preferences.read(|p| p.dj.auto_gain));
    lock(&state.tracks)[deck] = Some(track);
    for c in commands.into_iter().chain([Command::TakePosition { deck, from }]) {
        state.send_if_running(c)?;
    }
    Ok(info)
}

/// The waveform of the track on `deck`, in binary (`dto::waveform_bytes`); empty when
/// it is not known yet or another track was loaded meanwhile.
#[tauri::command]
pub fn deck_waveform(state: State<'_, AppState>, deck: usize, load_id: u64) -> Response {
    let bytes = state.with_loaded(deck, load_id, |t| t.waveform.as_ref().map(waveform_bytes)).flatten();
    Response::new(bytes.unwrap_or_default())
}

/// Where each part of the separated track on `deck` plays (`prisme_analysis::stem_activity`):
/// a row of levels for the vocals then the instrumental, or for the vocals, drums, bass and
/// other instruments, one after the other; empty until the track is all separated.
#[tauri::command(async)]
pub fn stem_activity(state: State<'_, AppState>, deck: usize, load_id: u64) -> Response {
    let parts = state.with_loaded(deck, load_id, |t| {
        let stems: &[Stem] = match SeparationDto::of(|s| t.stems[s as usize].is_some()) {
            SeparationDto::None => return None,
            SeparationDto::Vocals => &[Stem::Vocals],
            SeparationDto::Full => &Stem::ALL,
        };
        let stems: Option<Vec<_>> = stems.iter().map(|&s| t.stems[s as usize].clone()).collect();
        Some((t.audio.clone(), stems?))
    });
    let rows = parts.flatten().and_then(|(audio, stems)| {
        prisme_analysis::stem_activity(&audio, &stems.iter().map(|s| &**s).collect::<Vec<_>>())
    });
    Response::new(rows.map(|r| r.concat()).unwrap_or_default())
}

/// Tracks currently loaded on the decks (to rebuild the UI after a reload).
#[tauri::command]
pub fn loaded_tracks(state: State<'_, AppState>) -> Vec<Option<TrackInfoDto>> {
    lock(&state.tracks).iter().enumerate().map(|(deck, t)| t.as_ref().map(|t| t.info(deck))).collect()
}
