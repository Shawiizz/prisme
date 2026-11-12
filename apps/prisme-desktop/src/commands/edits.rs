//! Corrections of a loaded track's beat grid and key: stored in the library, applied to
//! the engine and shown in the library lists at once.

use crate::dto::{DeckTrackUpdateDto, KeyDto, TrackAnalysisDto};
use crate::error::{AppError, AppResult};
use crate::library_service::LibraryService;
use crate::state::AppState;
use crate::track_loading::DECK_TRACK_UPDATE_EVENT;
use prisme_analysis::{Key, KeyEstimate};
use prisme_engine::Command;
use prisme_library::{GridEdit, TrackEdits};
use serde::Deserialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GridChange {
    /// Another tempo, anchored on the first beat.
    Bpm { bpm: f64 },
    /// Double or half time.
    Scale { factor: f64 },
    /// Moves the grid (later when positive).
    Shift { seconds: f64 },
    /// Puts a beat where the deck is.
    BeatHere,
    /// Back to the analysis.
    Reset,
}

#[tauri::command]
pub fn edit_grid(
    app: AppHandle,
    state: State<'_, AppState>,
    library: State<'_, Arc<LibraryService>>,
    deck: usize,
    change: GridChange,
) -> AppResult<TrackAnalysisDto> {
    let position_s = match change {
        GridChange::BeatHere => state.deck_status(deck)?.position_s,
        _ => 0.0,
    };
    edit(&app, &state, &library, deck, |track_grid, edits| {
        let grid = || track_grid.ok_or(AppError::NoBeatGrid(deck));
        let with_bpm = |g: GridEdit, bpm: f64| g.with_bpm(bpm).ok_or(AppError::InvalidBpm(bpm));
        edits.grid = match change {
            GridChange::Bpm { bpm } => {
                Some(with_bpm(track_grid.unwrap_or(GridEdit { bpm, first_beat_s: 0.0 }), bpm)?)
            }
            GridChange::Scale { factor } => Some(with_bpm(grid()?, grid()?.bpm * factor)?),
            GridChange::Shift { seconds } => Some(grid()?.shifted(seconds)),
            GridChange::BeatHere => Some(grid()?.with_beat_at(position_s)),
            GridChange::Reset => None,
        };
        Ok(())
    })
}

/// Sets the key from its Camelot code; `None` goes back to the detected key.
#[tauri::command]
pub fn set_key(
    app: AppHandle,
    state: State<'_, AppState>,
    library: State<'_, Arc<LibraryService>>,
    deck: usize,
    camelot: Option<String>,
) -> AppResult<TrackAnalysisDto> {
    let key = camelot.map(|code| Key::from_camelot(&code).ok_or(AppError::InvalidKey(code))).transpose()?;
    edit(&app, &state, &library, deck, |_, edits| {
        edits.key = key;
        Ok(())
    })
}

/// The 24 keys, in Camelot order.
#[tauri::command]
pub fn key_choices() -> Vec<KeyDto> {
    Key::all().map(|key| KeyDto::from(&KeyEstimate { key, confidence: 1.0, tuning_cents: 0.0 })).collect()
}

/// Applies `change` to the stored edits of the track on `deck` (given its grid with them),
/// stores them, and updates every deck playing the track, the engine and the UI. The
/// stored edits, not the deck's: another deck may have changed them since it loaded.
fn edit(
    app: &AppHandle,
    state: &AppState,
    library: &LibraryService,
    deck: usize,
    change: impl FnOnce(Option<GridEdit>, &mut TrackEdits) -> AppResult<()>,
) -> AppResult<TrackAnalysisDto> {
    let (path, detected) = state.with_track(deck, |track| (track.path.clone(), track.detected))?;
    let mut edits = library.db.edits(&path)?;
    change(edits.apply(detected).tempo.map(GridEdit::from), &mut edits)?;
    library.db.save_edits(&path, &edits)?;
    let updated = state.with_path(&path, |track| {
        track.edits = edits;
        (track.grid(), TrackAnalysisDto::from(&*track))
    });
    let mut analysis = None;
    for (on, load_id, (grid, dto)) in updated {
        state.send_if_running(Command::SetBeatGrid { deck: on, grid })?;
        if on == deck {
            analysis = Some(dto);
        } else {
            let update = DeckTrackUpdateDto {
                deck: on,
                load_id,
                waveform_ready: false,
                analysis: Some(dto),
                separation: None,
            };
            let _ = app.emit(DECK_TRACK_UPDATE_EVENT, update);
        }
    }
    library.announce(&path);
    library.write_tags_soon();
    analysis.ok_or(AppError::NoTrack(deck))
}
