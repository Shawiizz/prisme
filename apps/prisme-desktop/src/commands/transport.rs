use crate::actions::pads;
use crate::error::AppResult;
use crate::state::AppState;
use tauri::State;

/// The slot of the chain of `deck` the screen's pads and knobs act on.
#[tauri::command]
pub fn show_fx_slot(state: State<'_, AppState>, deck: usize, slot: usize) -> AppResult<()> {
    crate::actions::fx::show_fx_slot(&state, deck, slot)
}

/// The slicer of `deck` put on the bar the track is in, as its pads are shown.
#[tauri::command]
pub fn anchor_slicer(state: State<'_, AppState>, deck: usize) -> AppResult<()> {
    pads::anchor_slicer(&state, deck)
}
