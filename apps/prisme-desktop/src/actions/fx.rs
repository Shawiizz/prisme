//! A deck's chain of beat effects: each slot's effect and settings, and the chain on or off.

use crate::dto::FxDto;
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState};
use prisme_engine::{Command, FX_SLOTS};

/// Changes a slot of the deck's chain as the backend keeps it, then sends `command` to the
/// engine.
pub fn set_slot(
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
