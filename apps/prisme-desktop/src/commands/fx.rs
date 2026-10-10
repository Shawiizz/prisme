//! A deck's beat effect.

use crate::dto::EffectKindDto;
use crate::error::AppResult;
use crate::state::{lock, AppState};
use prisme_engine::Command;
use tauri::State;

#[tauri::command]
pub fn set_fx(state: State<'_, AppState>, deck: usize, kind: EffectKindDto) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].fx.kind = kind;
    state.send_if_running(Command::SetFx { deck, kind: kind.into() })
}

#[tauri::command]
pub fn set_fx_enabled(state: State<'_, AppState>, deck: usize, enabled: bool) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].fx.enabled = enabled;
    state.send_if_running(Command::SetFxEnabled { deck, enabled })
}

/// Level of the effect, 0..1.
#[tauri::command]
pub fn set_fx_mix(state: State<'_, AppState>, deck: usize, mix: f32) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].fx.mix = mix;
    state.send_if_running(Command::SetFxMix { deck, mix })
}

/// The effect's main knob, 0..1.
#[tauri::command]
pub fn set_fx_amount(state: State<'_, AppState>, deck: usize, amount: f32) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].fx.amount = amount;
    state.send_if_running(Command::SetFxAmount { deck, amount })
}

/// The effect's period in beats.
#[tauri::command]
pub fn set_fx_beats(state: State<'_, AppState>, deck: usize, beats: f32) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].fx.beats = beats;
    state.send_if_running(Command::SetFxBeats { deck, beats })
}
