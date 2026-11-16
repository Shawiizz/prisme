use crate::error::AppResult;
use crate::state::AppState;
use prisme_engine::{Command, LoopRegion};
use tauri::State;

/// Temporary speed change while a bend key is held (0 when released).
#[tauri::command]
pub fn set_pitch_bend(state: State<'_, AppState>, deck: usize, amount: f64) -> AppResult<()> {
    send(&state, deck, Command::SetPitchBend { deck, amount })
}

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
pub fn loop_in(state: State<'_, AppState>, deck: usize) -> AppResult<()> {
    send(&state, deck, Command::LoopIn { deck })
}

#[tauri::command]
pub fn loop_out(state: State<'_, AppState>, deck: usize) -> AppResult<()> {
    send(&state, deck, Command::LoopOut { deck })
}

/// Plays the slice from `start_s`, `length_s` long, round and round while held, the track
/// going on underneath; `None`: release.
#[tauri::command]
pub fn slice(state: State<'_, AppState>, deck: usize, start_s: Option<f64>, length_s: f64) -> AppResult<()> {
    let region = start_s.map(|start_s| LoopRegion { start_s, end_s: start_s + length_s });
    send(&state, deck, Command::Slice { deck, region })
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

/// A playing deck slows to a stop as a turntable switched off; a stopped one starts softly.
#[tauri::command]
pub fn brake(state: State<'_, AppState>, deck: usize) -> AppResult<()> {
    send(&state, deck, Command::Brake { deck })
}

/// The record spun back fast while `held`; let go, it plays on from there.
#[tauri::command]
pub fn spin_back(state: State<'_, AppState>, deck: usize, held: bool) -> AppResult<()> {
    send(&state, deck, Command::SpinBack { deck, held })
}

fn send(state: &AppState, deck: usize, command: Command) -> AppResult<()> {
    state.has_deck(deck)?;
    state.send(command)
}
