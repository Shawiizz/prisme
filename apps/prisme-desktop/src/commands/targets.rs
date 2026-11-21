//! The actions a key or a controller can drive, as the interface lists them: for the keys, the
//! controllers' mappings and the command palette.

use crate::dto::TargetDto;
use crate::state::AppState;
use prisme_control::Target;
use tauri::State;

/// Every action listed, in the order shown.
#[tauri::command]
pub fn list_targets(state: State<'_, AppState>) -> Vec<TargetDto> {
    let decks = state.decks_in_use();
    Target::all().iter().map(|target| TargetDto::of(target, decks)).collect()
}
