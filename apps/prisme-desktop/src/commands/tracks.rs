use crate::auto_dj::{AutoDj, Takeover};
use crate::dto::{SeparationDto, TrackInfoDto};
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState};
use crate::track_loading;
use prisme_engine::{Command, Stem};
use std::path::PathBuf;
use tauri::ipc::{Channel, Response};
use tauri::{AppHandle, State};

/// Puts a track on a deck as soon as it can play; its waveform and analysis follow if
/// they were not known (see `track_loading`). The Auto DJ gives way. `None` if another load
/// was asked for on the deck meanwhile.
#[tauri::command]
pub async fn load_track(
    app: AppHandle,
    state: State<'_, AppState>,
    auto_dj: State<'_, AutoDj>,
    deck: usize,
    path: String,
) -> AppResult<Option<TrackInfoDto>> {
    state.has_deck(deck)?;
    auto_dj.take_over(Takeover::Load(deck));
    let file = PathBuf::from(&path);
    tauri::async_runtime::spawn_blocking(move || track_loading::load_on_deck(&app, deck, file))
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
}

/// Puts the track of deck `from` on `deck` where it is, playing if it plays (an instant
/// double): nothing is read again. `None` if another load was asked for on the deck meanwhile.
#[tauri::command]
pub fn double_deck(
    state: State<'_, AppState>,
    auto_dj: State<'_, AutoDj>,
    from: usize,
    deck: usize,
) -> AppResult<Option<TrackInfoDto>> {
    state.has_deck(from)?;
    state.has_deck(deck)?;
    auto_dj.take_over(Takeover::Load(deck));
    let turn = state.ask_load(deck);
    let track = state.with_track(from, |t| t.double())?;
    let info = track.info(deck);
    let auto_gain = state.preferences.read(|p| p.dj.auto_gain);
    let commands =
        track.load_commands(deck, auto_gain).into_iter().chain([Command::TakePlace { deck, from }]);
    Ok(state.put_track(deck, turn, Some(track), commands)?.then_some(info))
}

/// Sends the waveform of the track on `deck` to `channel` (`dto::waveform_bytes`): what is
/// known of it at once, then, while the track is decoded, each piece computed. Nothing once
/// another track was loaded.
#[tauri::command]
pub fn watch_waveform(state: State<'_, AppState>, deck: usize, load_id: u64, channel: Channel) {
    state.with_loaded(deck, load_id, |t| t.waveform.watch(channel));
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

/// Takes the track off `deck`.
#[tauri::command]
pub fn eject_track(state: State<'_, AppState>, deck: usize) -> AppResult<()> {
    state.has_deck(deck)?;
    let turn = state.ask_load(deck);
    state.put_track(deck, turn, None, [Command::Eject { deck }]).map(drop)
}
