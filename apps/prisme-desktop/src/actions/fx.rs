//! A deck's chain of beat effects: each slot's effect and settings, and the chain on or off.

use crate::dto::{EffectKindDto, FxDto};
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState};
use prisme_engine::{Command, FX_SLOTS};

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

pub fn set_fx_enabled(state: &AppState, deck: usize, slot: usize, enabled: bool) -> AppResult<()> {
    set_slot(state, deck, slot, |fx| fx.enabled = enabled, Command::SetFxEnabled { deck, slot, enabled })
}

/// Another effect in the slot, on or off as it was.
pub fn set_fx(state: &AppState, deck: usize, slot: usize, kind: EffectKindDto) -> AppResult<()> {
    set_slot(state, deck, slot, |fx| fx.kind = kind, Command::SetFx { deck, slot, kind: kind.into() })
}

/// The level of the slot's effect, 0..1.
pub fn set_fx_mix(state: &AppState, deck: usize, slot: usize, mix: f32) -> AppResult<()> {
    set_slot(state, deck, slot, |fx| fx.mix = mix, Command::SetFxMix { deck, slot, mix })
}

/// The slot's main knob, 0..1.
pub fn set_fx_amount(state: &AppState, deck: usize, slot: usize, amount: f32) -> AppResult<()> {
    set_slot(state, deck, slot, |fx| fx.amount = amount, Command::SetFxAmount { deck, slot, amount })
}

/// The slot's second knob, 0..1.
pub fn set_fx_param2(state: &AppState, deck: usize, slot: usize, param2: f32) -> AppResult<()> {
    set_slot(state, deck, slot, |fx| fx.param2 = param2, Command::SetFxParam2 { deck, slot, param2 })
}

/// The slot's period in beats.
pub fn set_fx_beats(state: &AppState, deck: usize, slot: usize, beats: f32) -> AppResult<()> {
    set_slot(state, deck, slot, |fx| fx.beats = beats, Command::SetFxBeats { deck, slot, beats })
}

/// The whole chain on or off, each slot keeping its own switch.
pub fn set_fx_chain_enabled(state: &AppState, deck: usize, enabled: bool) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].fx.enabled = enabled;
    state.send_if_running(Command::SetFxChainEnabled { deck, enabled })
}
