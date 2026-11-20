//! The software mixer: the decks' channels and what they are mixed into.

use crate::dto::BandDto;
use crate::error::AppResult;
use crate::state::{lock, AppState};
use prisme_engine::Command;

/// A band of the deck's EQ cut, or back.
pub fn set_eq_kill(state: &AppState, deck: usize, band: BandDto, kill: bool) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].strip.kills[band as usize] = kill;
    state.send_if_running(Command::SetEqKill { deck, band: band.into(), kill })
}
