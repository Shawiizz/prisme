use crate::actions::pads;
use crate::actions::transport::{self, send};
use crate::auto_dj::AutoDj;
use crate::error::AppResult;
use crate::state::AppState;
use prisme_engine::Command;
use tauri::State;

#[tauri::command]
pub fn beat_loop(state: State<'_, AppState>, deck: usize, beats: f64) -> AppResult<()> {
    send(&state, deck, Command::BeatLoop { deck, beats })
}

#[tauri::command]
pub fn loop_in(state: State<'_, AppState>, deck: usize) -> AppResult<()> {
    send(&state, deck, Command::LoopIn { deck })
}

#[tauri::command]
pub fn loop_out(state: State<'_, AppState>, deck: usize) -> AppResult<()> {
    send(&state, deck, Command::LoopOut { deck })
}

/// The slicer of `deck` put on the bar the track is in, as its pads are shown.
#[tauri::command]
pub fn anchor_slicer(state: State<'_, AppState>, deck: usize) -> AppResult<()> {
    pads::anchor_slicer(&state, deck)
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
pub fn beat_jump(
    state: State<'_, AppState>,
    auto_dj: State<'_, AutoDj>,
    deck: usize,
    beats: f64,
) -> AppResult<()> {
    transport::beat_jump(&state, Some(&auto_dj), deck, beats)
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

/// The track played backwards, or forwards again.
#[tauri::command]
pub fn set_reverse(state: State<'_, AppState>, deck: usize, enabled: bool) -> AppResult<()> {
    send(&state, deck, Command::SetReverse { deck, enabled })
}
