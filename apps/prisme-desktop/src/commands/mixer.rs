//! The mixer's outputs and its limiter.

use crate::actions::mixer::route;
use crate::dto::HeadphonesOutputDto;
use crate::error::AppResult;
use crate::state::AppState;
use prisme_engine::Command;
use tauri::State;

/// Where the headphones go, kept for the next start.
#[tauri::command]
pub fn set_headphones_output(state: State<'_, AppState>, output: HeadphonesOutputDto) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.headphones = output);
    route(&state)
}

/// The booth monitors' outputs (the first of the pair), kept for the next start.
#[tauri::command]
pub fn set_booth_output(state: State<'_, AppState>, first: Option<usize>) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.booth = first);
    route(&state)
}

/// Master limiter; kept for the next start.
#[tauri::command]
pub fn set_limiter(state: State<'_, AppState>, enabled: bool) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.limiter = enabled);
    state.send_if_running(Command::SetLimiter { enabled })
}
