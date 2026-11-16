//! A deck's chain of beat effects: each slot's effect and settings, and the part of the track
//! the chain is on.

use crate::dto::{EffectKindDto, FxDto, FxPartDto};
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState};
use prisme_engine::{Command, FX_SLOTS};
use tauri::State;

/// Changes a slot of the deck's chain as the backend keeps it, then sends `command` to the
/// engine.
fn set_slot(
    state: &AppState,
    deck: usize,
    slot: usize,
    change: impl FnOnce(&mut FxDto),
    command: Command,
) -> AppResult<()> {
    AppState::check_deck(deck)?;
    if slot >= FX_SLOTS {
        return Err(AppError::Other(format!("no effect slot {}", slot + 1)));
    }
    change(&mut lock(&state.decks)[deck].fx.slots[slot]);
    state.send_if_running(command)
}

#[tauri::command]
pub fn set_fx(state: State<'_, AppState>, deck: usize, slot: usize, kind: EffectKindDto) -> AppResult<()> {
    set_slot(&state, deck, slot, |fx| fx.kind = kind, Command::SetFx { deck, slot, kind: kind.into() })
}

#[tauri::command]
pub fn set_fx_enabled(state: State<'_, AppState>, deck: usize, slot: usize, enabled: bool) -> AppResult<()> {
    set_slot(&state, deck, slot, |fx| fx.enabled = enabled, Command::SetFxEnabled { deck, slot, enabled })
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
