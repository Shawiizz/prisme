//! A deck's chain of beat effects: each slot's effect and settings, and the part of the track
//! the chain is on.

use crate::actions::fx;
use crate::dto::{EffectKindDto, FxPartDto};
use crate::error::AppResult;
use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub fn set_fx(state: State<'_, AppState>, deck: usize, slot: usize, kind: EffectKindDto) -> AppResult<()> {
    fx::set_fx(&state, deck, slot, kind)
}

#[tauri::command]
pub fn set_fx_enabled(state: State<'_, AppState>, deck: usize, slot: usize, enabled: bool) -> AppResult<()> {
    fx::set_fx_enabled(&state, deck, slot, enabled)
}

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

/// The part of the track the chain is on, where separated (`None`: the whole deck).
#[tauri::command]
pub fn set_fx_part(state: State<'_, AppState>, deck: usize, part: Option<FxPartDto>) -> AppResult<()> {
    fx::set_fx_part(&state, deck, part)
}
