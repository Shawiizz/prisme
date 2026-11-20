//! What the DJ sets for every deck: quantize.

use crate::error::AppResult;
use crate::state::AppState;
use prisme_engine::Command;

/// Loops, jumps, cues and samples on the beat, or not; kept for the next start.
pub fn set_quantize(state: &AppState, enabled: bool) -> AppResult<()> {
    state.update_preferences(|p| p.dj.quantize = enabled);
    state.send_if_running(Command::SetQuantize { enabled })
}
