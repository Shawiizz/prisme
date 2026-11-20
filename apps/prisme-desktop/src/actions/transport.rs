//! A deck's transport and loops as the engine plays them: what the controllers' keys and the
//! interface's buttons both send.

use super::take_over;
use crate::auto_dj::{AutoDj, Takeover};
use crate::error::AppResult;
use crate::state::AppState;
use prisme_engine::{Command, LoopRegion};

/// The speed change while a bend key is held, and while a small one is.
pub const PITCH_BEND: f64 = 0.03;
pub const SMALL_BEND: f64 = 0.01;

/// Sends `command` to the engine for `deck`, one of those in use.
pub fn send(state: &AppState, deck: usize, command: Command) -> AppResult<()> {
    state.has_deck(deck)?;
    state.send(command)
}

/// So many beats ahead, or back below zero; the Auto DJ lets the deck go.
pub fn beat_jump(state: &AppState, auto_dj: Option<&AutoDj>, deck: usize, beats: f64) -> AppResult<()> {
    take_over(auto_dj, Takeover::Transport(deck));
    send(state, deck, Command::BeatJump { deck, beats })
}

/// Stores the loop of `deck` from `start_s` to `end_s`, playing it if `active`; without them,
/// forgets it.
pub fn set_loop(
    state: &AppState,
    deck: usize,
    start_s: Option<f64>,
    end_s: Option<f64>,
    active: bool,
) -> AppResult<()> {
    let region = start_s.zip(end_s).filter(|(s, e)| s.is_finite() && e > s);
    let region = region.map(|(start_s, end_s)| LoopRegion { start_s, end_s });
    send(state, deck, Command::SetLoop { deck, region, active: active && region.is_some() })
}
