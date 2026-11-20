//! A deck's chain of beat effects: each slot's effect and settings, and the part of the track
//! the chain is on.

use crate::actions::fx::{self, set_slot};
use crate::dto::{EffectKindDto, FxPartDto};
use crate::error::AppResult;
use crate::state::{lock, AppState};
use prisme_engine::Command;
use tauri::State;

#[tauri::command]
pub fn set_fx(state: State<'_, AppState>, deck: usize, slot: usize, kind: EffectKindDto) -> AppResult<()> {
    set_slot(&state, deck, slot, |fx| fx.kind = kind, Command::SetFx { deck, slot, kind: kind.into() })
}

#[tauri::command]
pub fn set_fx_enabled(state: State<'_, AppState>, deck: usize, slot: usize, enabled: bool) -> AppResult<()> {
    fx::set_fx_enabled(&state, deck, slot, enabled)
}

/// Level of the slot's effect, 0..1.
#[tauri::command]
pub fn set_fx_mix(state: State<'_, AppState>, deck: usize, slot: usize, mix: f32) -> AppResult<()> {
    set_slot(&state, deck, slot, |fx| fx.mix = mix, Command::SetFxMix { deck, slot, mix })
}

/// The slot's main knob, 0..1.
#[tauri::command]
pub fn set_fx_amount(state: State<'_, AppState>, deck: usize, slot: usize, amount: f32) -> AppResult<()> {
    set_slot(&state, deck, slot, |fx| fx.amount = amount, Command::SetFxAmount { deck, slot, amount })
}

/// The slot's second knob, 0..1.
#[tauri::command]
pub fn set_fx_param2(state: State<'_, AppState>, deck: usize, slot: usize, param2: f32) -> AppResult<()> {
    set_slot(&state, deck, slot, |fx| fx.param2 = param2, Command::SetFxParam2 { deck, slot, param2 })
}

/// The slot's period in beats.
#[tauri::command]
pub fn set_fx_beats(state: State<'_, AppState>, deck: usize, slot: usize, beats: f32) -> AppResult<()> {
    set_slot(&state, deck, slot, |fx| fx.beats = beats, Command::SetFxBeats { deck, slot, beats })
}

/// The whole chain on or off, each slot keeping its own switch.
#[tauri::command]
pub fn set_fx_chain_enabled(state: State<'_, AppState>, deck: usize, enabled: bool) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].fx.enabled = enabled;
    state.send_if_running(Command::SetFxChainEnabled { deck, enabled })
}

/// The part of the track the chain is on, where separated (`None`: the whole deck).
#[tauri::command]
pub fn set_fx_part(state: State<'_, AppState>, deck: usize, part: Option<FxPartDto>) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].fx.part = part;
    state.send_if_running(Command::SetFxParts { deck, parts: FxPartDto::parts(part) })
}
