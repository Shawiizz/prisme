//! Corrections of a loaded track's beat grid and key (see `actions::edits`), the grid editor's
//! taps, the keys offered.

use crate::actions::edits::{change_grid, change_key, GridChange};
use crate::actions::Actions;
use crate::dto::KeyDto;
use crate::error::{AppError, AppResult};
use crate::library_service::LibraryService;
use crate::state::AppState;
use prisme_analysis::Key;
use std::sync::Arc;
use tauri::{AppHandle, State};

#[tauri::command]
pub fn edit_grid(
    app: AppHandle,
    state: State<'_, AppState>,
    library: State<'_, Arc<LibraryService>>,
    deck: usize,
    change: GridChange,
) -> AppResult<()> {
    change_grid(&app, &state, &library, deck, change)
}

/// Sets the key from its Camelot code; `None` goes back to the detected key.
#[tauri::command]
pub fn set_key(
    app: AppHandle,
    state: State<'_, AppState>,
    library: State<'_, Arc<LibraryService>>,
    deck: usize,
    camelot: Option<String>,
) -> AppResult<()> {
    let key = camelot.map(|code| Key::from_camelot(&code).ok_or(AppError::InvalidKey(code))).transpose()?;
    change_key(&app, &state, &library, deck, key)
}

/// A tap of the grid editor's tempo key for the track on `deck`: the tempo tapped, once there
/// are enough taps, for the DJ to apply.
#[tauri::command]
pub fn tap_tempo(actions: State<'_, Actions>, deck: usize) -> AppResult<Option<f64>> {
    AppState::deck_exists(deck)?;
    Ok(actions.tapped(deck, actions.now_s()))
}

/// The 24 keys, in Camelot order.
#[tauri::command]
pub fn key_choices() -> Vec<KeyDto> {
    Key::all().map(KeyDto::from).collect()
}
