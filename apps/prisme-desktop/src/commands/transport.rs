use crate::actions::pads;
use crate::actions::transport::send;
use crate::error::AppResult;
use crate::state::AppState;
use prisme_engine::Command;
use tauri::State;

/// The slicer of `deck` put on the bar the track is in, as its pads are shown.
#[tauri::command]
pub fn anchor_slicer(state: State<'_, AppState>, deck: usize) -> AppResult<()> {
    pads::anchor_slicer(&state, deck)
}

#[tauri::command]
pub fn exit_loop(state: State<'_, AppState>, deck: usize) -> AppResult<()> {
    send(&state, deck, Command::ExitLoop { deck })
}
