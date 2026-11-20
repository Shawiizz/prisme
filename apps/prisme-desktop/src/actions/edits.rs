//! What the actions change of a deck's track in the library: the beat grid corrected, at once
//! on the decks and kept in the library in turn (see `commands::edits`).

use super::Performing;
use crate::commands::edits::{change_grid, GridChange};
use crate::error::{AppError, AppResult};
use crate::library_service::LibraryService;
use prisme_actions::input::Given;
use prisme_control::DeckAction;
use std::sync::Arc;
use tauri::Manager;

/// A step of a beat grid moved (s), and stretched (BPM).
const GRID_STEP_S: f64 = 0.005;
const GRID_STEP_BPM: f64 = 0.01;

impl Performing<'_> {
    /// The library, reached through the application.
    fn library_service(&self) -> AppResult<(&tauri::AppHandle, tauri::State<'_, Arc<LibraryService>>)> {
        let app = self.app.ok_or_else(|| AppError::Other("no library here".into()))?;
        let library =
            app.try_state::<Arc<LibraryService>>().ok_or_else(|| AppError::Other("no library yet".into()))?;
        Ok((app, library))
    }

    /// Corrects the beat grid of the track on `deck` as `action` given `given` does.
    pub(super) fn edit_grid(&self, deck: usize, action: DeckAction, given: Given) -> AppResult<()> {
        let bpm = || self.state.with_track(deck, |t| t.grid().map(|g| g.bpm)).ok().flatten();
        match grid_change(action, given, bpm) {
            Some(change) => self.change_grid(deck, change),
            None => Ok(()),
        }
    }

    /// Corrects the beat grid of the track on `deck` by `change`.
    pub(super) fn change_grid(&self, deck: usize, change: GridChange) -> AppResult<()> {
        let (app, library) = self.library_service()?;
        change_grid(app, self.state, &library, deck, change)
    }
}

/// The correction `action` given `given` makes of a grid of `bpm`: none for a key let go, nor
/// a tempo changed on a track with no grid.
fn grid_change(action: DeckAction, given: Given, bpm: impl FnOnce() -> Option<f64>) -> Option<GridChange> {
    let shift = |steps: f64| GridChange::Shift { seconds: steps * GRID_STEP_S };
    let stretch = |steps: f64| bpm().map(|bpm| GridChange::Bpm { bpm: bpm + steps * GRID_STEP_BPM });
    match (action, given) {
        (DeckAction::GridMove, Given::Steps(steps)) => Some(shift(f64::from(steps))),
        _ if !given.pressed() => None,
        (DeckAction::GridHere, _) => Some(GridChange::BeatHere),
        (DeckAction::GridEarlier, _) => Some(shift(-1.0)),
        (DeckAction::GridLater, _) => Some(shift(1.0)),
        (DeckAction::GridFaster, _) => stretch(1.0),
        (DeckAction::GridSlower, _) => stretch(-1.0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(id: &str, given: Given, bpm: Option<f64>) -> Option<GridChange> {
        let prisme_control::Target::Deck(_, action) = id.parse().unwrap() else { unreachable!() };
        grid_change(action, given, || bpm)
    }

    #[test]
    fn the_grid_is_put_on_a_beat_moved_by_a_few_milliseconds_or_stretched_by_a_hundredth() {
        let pressed = Given::Value(1.0);
        assert!(matches!(change("deck1.grid.here", pressed, None), Some(GridChange::BeatHere)));
        let earlier = change("deck1.grid.earlier", pressed, None);
        assert!(matches!(earlier, Some(GridChange::Shift { seconds }) if seconds == -0.005));
        let moved = change("deck1.grid.move", Given::Steps(3), None);
        assert!(matches!(moved, Some(GridChange::Shift { seconds }) if (seconds - 0.015).abs() < 1e-12));
        let faster = change("deck1.grid.faster", pressed, Some(120.0));
        assert!(matches!(faster, Some(GridChange::Bpm { bpm }) if (bpm - 120.01).abs() < 1e-9));
        assert!(change("deck1.grid.slower", pressed, None).is_none(), "no grid to stretch");
        assert!(change("deck1.grid.later", Given::Value(0.0), None).is_none(), "let go");
    }
}
