use crate::error::AppResult;
use crate::state::AppState;
use prisme_engine::Command;
use tauri::State;

#[tauri::command]
pub fn beat_loop(state: State<'_, AppState>, deck: usize, beats: f64) -> AppResult<()> {
    send(&state, deck, Command::BeatLoop { deck, beats })
}

/// `beats`: start a roll of that length; `None`: release it.
#[tauri::command]
pub fn loop_roll(state: State<'_, AppState>, deck: usize, beats: Option<f64>) -> AppResult<()> {
    send(&state, deck, Command::LoopRoll { deck, beats })
}

#[tauri::command]
pub fn exit_loop(state: State<'_, AppState>, deck: usize) -> AppResult<()> {
    send(&state, deck, Command::ExitLoop { deck })
}

#[tauri::command]
pub fn reloop(state: State<'_, AppState>, deck: usize) -> AppResult<()> {
    send(&state, deck, Command::Reloop { deck })
}

#[tauri::command]
pub fn scale_loop(state: State<'_, AppState>, deck: usize, factor: f64) -> AppResult<()> {
    send(&state, deck, Command::ScaleLoop { deck, factor })
}

#[tauri::command]
pub fn beat_jump(state: State<'_, AppState>, deck: usize, beats: f64) -> AppResult<()> {
    send(&state, deck, Command::BeatJump { deck, beats })
}

#[tauri::command]
pub fn set_slip(state: State<'_, AppState>, deck: usize, enabled: bool) -> AppResult<()> {
    send(&state, deck, Command::SetSlip { deck, enabled })
}

#[tauri::command]
pub fn censor(state: State<'_, AppState>, deck: usize, active: bool) -> AppResult<()> {
    send(&state, deck, Command::Censor { deck, active })
}

fn send(state: &AppState, deck: usize, command: Command) -> AppResult<()> {
    AppState::check_deck(deck)?;
    state.send(command)
}
