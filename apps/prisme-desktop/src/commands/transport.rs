use crate::actions::pads;
use crate::error::AppResult;
use crate::state::AppState;
use tauri::State;

/// The slicer of `deck` put on the bar the track is in, as its pads are shown.
#[tauri::command]
pub fn anchor_slicer(state: State<'_, AppState>, deck: usize) -> AppResult<()> {
    pads::anchor_slicer(&state, deck)
}
