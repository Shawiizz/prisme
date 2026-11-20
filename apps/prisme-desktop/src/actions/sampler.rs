//! A deck's sampler: its sounds played and stopped (see `crate::sampler`).

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use prisme_engine::{Command, SAMPLE_SLOTS};

pub fn check_slot(deck: usize, slot: usize) -> AppResult<()> {
    AppState::check_deck(deck)?;
    if slot < SAMPLE_SLOTS {
        Ok(())
    } else {
        Err(AppError::Other(format!("no sampler slot {}", slot + 1)))
    }
}

/// Plays a slot from its start (on the next beat with quantize on), as loud as its pad was
/// hit (`velocity` 0-1) times the sampler's level, or at that level.
pub fn trigger_sample(state: &AppState, deck: usize, slot: usize, velocity: Option<f32>) -> AppResult<()> {
    check_slot(deck, slot)?;
    let gain = velocity.unwrap_or(1.0).clamp(0.0, 1.0);
    state.send(Command::TriggerSample { deck, slot, gain })
}

pub fn stop_sample(state: &AppState, deck: usize, slot: usize) -> AppResult<()> {
    check_slot(deck, slot)?;
    state.send(Command::StopSample { deck, slot })
}
