//! Corrections of a loaded track's beat grid and key: stored in the library, applied to
//! the engine and shown in the library lists at once.

use crate::dto::{KeyDto, TrackAnalysisDto};
use crate::error::{AppError, AppResult};
use crate::library_service::LibraryService;
use crate::state::AppState;
use prisme_analysis::{Key, KeyEstimate};
use prisme_engine::Command;
use prisme_library::{GridEdit, TrackEdits};
use serde::Deserialize;
use std::sync::Arc;
use tauri::State;

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
    state: State<'_, AppState>,
    library: State<'_, Arc<LibraryService>>,
    deck: usize,
    change: GridChange,
) -> AppResult<TrackAnalysisDto> {
    let position_s = match change {
        GridChange::BeatHere => state.deck_status(deck)?.position_s,
        _ => 0.0,
    };
    edit(&state, &library, deck, |track_grid, edits| {
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
    state: State<'_, AppState>,
    library: State<'_, Arc<LibraryService>>,
    deck: usize,
    camelot: Option<String>,
) -> AppResult<TrackAnalysisDto> {
    let key = camelot.map(|code| Key::from_camelot(&code).ok_or(AppError::InvalidKey(code))).transpose()?;
    edit(&state, &library, deck, |_, edits| {
        edits.key = key;
        Ok(())
    })
}

/// The 24 keys, in Camelot order.
#[tauri::command]
pub fn key_choices() -> Vec<KeyDto> {
    Key::all().map(|key| KeyDto::from(&KeyEstimate { key, confidence: 1.0, tuning_cents: 0.0 })).collect()
}

/// Applies `change` to the edits of the track on `deck` (given its current grid), then
/// stores them and updates the engine and the UI.
fn edit(
    state: &AppState,
    library: &LibraryService,
    deck: usize,
    change: impl FnOnce(Option<GridEdit>, &mut TrackEdits) -> AppResult<()>,
) -> AppResult<TrackAnalysisDto> {
    let (path, grid, analysis) = state.with_track(deck, |track| {
        let mut edits = track.edits;
        change(track.analysis().tempo.map(GridEdit::from), &mut edits)?;
        library.db.save_edits(&track.path, &edits)?;
        track.edits = edits;
        Ok::<_, AppError>((track.path.clone(), track.grid(), TrackAnalysisDto::from(&*track)))
    })??;
    state.send_if_running(Command::SetBeatGrid { deck, grid })?;
    library.announce(&path);
    library.write_tags_soon();
    Ok(analysis)
}
