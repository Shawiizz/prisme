//! Corrections of a loaded track's beat grid and key: applied at once to every deck playing the
//! track and to the engine, then stored in the library in turn (see `Actions::keep`) and shown
//! in its lists; the decks playing the track show them with their tracks (`Section::Tracks`).

use super::{Actions, Performing};
use crate::error::{AppError, AppResult};
use crate::library_service::LibraryService;
use crate::state::{lock, AppState};
use prisme_actions::input::Given;
use prisme_actions::sync::grid_shift_to;
use prisme_actions::view::mixed_with;
use prisme_analysis::Key;
use prisme_control::DeckAction;
use prisme_engine::Command;
use prisme_library::{GridEdit, TrackEdits};
use serde::Deserialize;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tracing::error;

/// A step of a beat grid moved (s), and stretched (BPM).
const GRID_STEP_S: f64 = 0.005;
const GRID_STEP_BPM: f64 = 0.01;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum GridChange {
    /// Another tempo, anchored on the first beat.
    Bpm { bpm: f64 },
    /// Double or half time.
    Scale { factor: f64 },
    /// Moves the grid (later when positive).
    Shift { seconds: f64 },
    /// Puts a beat where the deck is.
    BeatHere,
    /// Starts a bar where the deck is, on a beat.
    BarHere,
    /// Pins the beat nearest the deck where it is: the grid follows a tempo that drifts.
    PinHere,
    /// Takes off the pin nearest the deck.
    Unpin,
    /// Moves the grid for its beats to fall on those of the deck it is mixed with, both playing.
    Align,
    /// Back to the analysis.
    Reset,
}

/// A [`GridChange`] as the deck it is made on resolves it: a beat put at a place, a grid
/// shifted onto another deck's beats by so many seconds.
#[derive(Debug, Clone, Copy)]
enum Regrid {
    Bpm(f64),
    Scale(f64),
    Shift(f64),
    BeatAt(f64),
    BarAt(f64),
    PinAt(f64),
    UnpinAt(f64),
    Reset,
}

impl GridChange {
    /// What it does to the grid of the track on `deck`: nothing for an alignment the decks do
    /// not allow.
    fn resolved(self, state: &AppState, deck: usize) -> AppResult<Option<Regrid>> {
        Ok(Some(match self {
            GridChange::Bpm { bpm } => Regrid::Bpm(bpm),
            GridChange::Scale { factor } => Regrid::Scale(factor),
            GridChange::Shift { seconds } => Regrid::Shift(seconds),
            GridChange::BeatHere => Regrid::BeatAt(state.deck_status(deck)?.position_s),
            GridChange::BarHere => Regrid::BarAt(state.deck_status(deck)?.position_s),
            GridChange::PinHere => Regrid::PinAt(state.deck_status(deck)?.position_s),
            GridChange::Unpin => Regrid::UnpinAt(state.deck_status(deck)?.position_s),
            GridChange::Align => match shift_to_other(state, deck)? {
                Some(seconds) => Regrid::Shift(seconds),
                None => return Ok(None),
            },
            GridChange::Reset => Regrid::Reset,
        }))
    }
}

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
        let bpm = || self.state.with_track(deck, |t| t.grid().map(|g| g.mean_bpm())).ok().flatten();
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
        (DeckAction::GridBar, _) => Some(GridChange::BarHere),
        (DeckAction::GridPin, _) => Some(GridChange::PinHere),
        (DeckAction::GridUnpin, _) => Some(GridChange::Unpin),
        (DeckAction::GridEarlier, _) => Some(shift(-1.0)),
        (DeckAction::GridLater, _) => Some(shift(1.0)),
        (DeckAction::GridFaster, _) => stretch(1.0),
        (DeckAction::GridSlower, _) => stretch(-1.0),
        _ => None,
    }
}

/// Corrects the beat grid of the track on `deck` by `change`.
pub fn change_grid(
    app: &AppHandle,
    state: &AppState,
    library: &Arc<LibraryService>,
    deck: usize,
    change: GridChange,
) -> AppResult<()> {
    let Some(regrid) = change.resolved(state, deck)? else { return Ok(()) };
    edit(app, state, library, deck, |track_grid, edits| {
        let grid = track_grid.ok_or(AppError::NoBeatGrid(deck));
        edits.grid = match regrid {
            Regrid::Bpm(bpm) => {
                Some(grid.unwrap_or(GridEdit::new(bpm, 0.0)).with_bpm(bpm).ok_or(AppError::InvalidBpm(bpm))?)
            }
            Regrid::Scale(factor) => {
                let grid = grid?;
                let bpm = grid.grid.mean_bpm() * factor;
                Some(grid.scaled(factor).ok_or(AppError::InvalidBpm(bpm))?)
            }
            Regrid::Shift(seconds) => Some(grid?.shifted(seconds)),
            Regrid::BeatAt(position_s) => Some(grid?.with_beat_at(position_s)),
            Regrid::BarAt(position_s) => Some(grid?.with_bar_at(position_s)),
            Regrid::PinAt(position_s) => Some(grid?.pinned_at(position_s).ok_or(AppError::PinRefused(deck))?),
            // no pin to take off: the grid as it is
            Regrid::UnpinAt(position_s) => {
                let grid = grid?;
                Some(grid.clone().unpinned_at(position_s).unwrap_or(grid))
            }
            Regrid::Reset => None,
        };
        Ok(())
    })
}

/// The key of the track on `deck` corrected to `key`, or back to the one detected.
pub fn change_key(
    app: &AppHandle,
    state: &AppState,
    library: &Arc<LibraryService>,
    deck: usize,
    key: Option<Key>,
) -> AppResult<()> {
    edit(app, state, library, deck, |_, edits| {
        edits.key = key;
        Ok(())
    })
}

/// How far to move the grid of the track on `deck` onto the beats of the deck it is mixed with,
/// both playing on a grid.
fn shift_to_other(state: &AppState, deck: usize) -> AppResult<Option<f64>> {
    let status = state.engine_status()?;
    let position_s = status.decks[deck].position_s;
    let Some(bpm) = state.with_track(deck, |t| t.grid().map(|g| g.bpm_at(position_s)))? else {
        return Ok(None);
    };
    let other = mixed_with(&status, deck);
    Ok(grid_shift_to(&status.decks[deck], &status.decks[other], bpm))
}

/// Applies `change` to the edits of the track on `deck` (given its grid with them), on every
/// deck playing the track, the engine and the interface (told of `deck` too: a controller may
/// have asked), then stores them. The decks playing a file all have its edits: they change
/// together, under one lock.
fn edit(
    app: &AppHandle,
    state: &AppState,
    library: &Arc<LibraryService>,
    deck: usize,
    change: impl FnOnce(Option<GridEdit>, &mut TrackEdits) -> AppResult<()>,
) -> AppResult<()> {
    AppState::deck_exists(deck)?;
    let (path, edits, updated) = {
        let mut tracks = lock(&state.tracks);
        let track = tracks[deck].as_ref().ok_or(AppError::NoTrack(deck))?;
        let (path, mut edits) = (track.path.clone(), track.edits.clone());
        change(track.grid().as_ref().map(GridEdit::from), &mut edits)?;
        let updated: Vec<_> = tracks
            .iter_mut()
            .enumerate()
            .filter_map(|(on, track)| {
                let track = track.as_mut().filter(|t| t.path == path)?;
                track.edits = edits.clone();
                Some((on, track.grid()))
            })
            .collect();
        (path, edits, updated)
    };
    for (on, grid) in updated {
        state.send_if_running(Command::SetBeatGrid { deck: on, grid })?;
    }
    let library = library.clone();
    app.state::<Actions>().keep(move || match library.db.save_edits(&path, &edits) {
        Ok(()) => {
            library.announce(&path);
            library.write_tags_soon();
        }
        Err(e) => error!("the corrections of {} not kept: {e}", path.display()),
    });
    Ok(())
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
        assert!(matches!(change("deck1.grid.bar", pressed, None), Some(GridChange::BarHere)));
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
