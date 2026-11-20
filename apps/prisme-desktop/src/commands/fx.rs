//! A deck's chain of beat effects as the screen's knobs and key set it: each slot's level and
//! knobs, the whole chain on or off.

use crate::actions::fx;
use crate::error::AppResult;
use crate::state::AppState;
use tauri::State;

/// Level of the slot's effect, 0..1.
#[tauri::command]
pub fn set_fx_mix(state: State<'_, AppState>, deck: usize, slot: usize, mix: f32) -> AppResult<()> {
    fx::set_fx_mix(&state, deck, slot, mix)
}

/// The slot's main knob, 0..1.
#[tauri::command]
pub fn set_fx_amount(state: State<'_, AppState>, deck: usize, slot: usize, amount: f32) -> AppResult<()> {
    fx::set_fx_amount(&state, deck, slot, amount)
}

/// The slot's second knob, 0..1.
#[tauri::command]
pub fn set_fx_param2(state: State<'_, AppState>, deck: usize, slot: usize, param2: f32) -> AppResult<()> {
    fx::set_fx_param2(&state, deck, slot, param2)
}

/// The whole chain on or off, each slot keeping its own switch.
#[tauri::command]
pub fn set_fx_chain_enabled(state: State<'_, AppState>, deck: usize, enabled: bool) -> AppResult<()> {
    fx::set_fx_chain_enabled(&state, deck, enabled)
}
