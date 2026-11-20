//! Corrections of a loaded track's beat grid and key: applied at once to every deck playing the
//! track and to the engine, then stored in the library in turn (see `Actions::keep`) and shown
//! in its lists; the decks playing the track show them as they are told (`deck-track-update`).

use crate::actions::Actions;
use crate::dto::{DeckTrackUpdateDto, KeyDto, TrackAnalysisDto};
use crate::error::{AppError, AppResult};
use crate::library_service::LibraryService;
use crate::state::{lock, AppState};
use crate::track_loading::DECK_TRACK_UPDATE_EVENT;
use prisme_actions::sync::grid_shift_to;
use prisme_actions::view::mixed_with;
use prisme_analysis::Key;
use prisme_engine::Command;
use prisme_library::{GridEdit, TrackEdits};
use serde::Deserialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tracing::error;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GridChange {
    /// Another tempo, anchored on the first beat.
    Bpm { bpm: f64 },
    /// Double or half time.
    Scale { factor: f64 },
    /// Moves the grid (later when positive).
    Shift { seconds: f64 },
    /// Puts a beat where the deck is.
    BeatHere,
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
            GridChange::Align => match shift_to_other(state, deck)? {
                Some(seconds) => Regrid::Shift(seconds),
                None => return Ok(None),
            },
            GridChange::Reset => Regrid::Reset,
        }))
    }
}

#[tauri::command]
pub fn edit_grid(
    app: AppHandle,
    state: State<'_, AppState>,
    library: State<'_, Arc<LibraryService>>,
    deck: usize,
    change: GridChange,
) -> AppResult<()> {
    change_grid(&app, &state, &library, deck, change)
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
        let grid = || track_grid.ok_or(AppError::NoBeatGrid(deck));
        let with_bpm = |g: GridEdit, bpm: f64| g.with_bpm(bpm).ok_or(AppError::InvalidBpm(bpm));
        edits.grid = match regrid {
            Regrid::Bpm(bpm) => {
                Some(with_bpm(track_grid.unwrap_or(GridEdit { bpm, first_beat_s: 0.0 }), bpm)?)
            }
            Regrid::Scale(factor) => Some(with_bpm(grid()?, grid()?.bpm * factor)?),
            Regrid::Shift(seconds) => Some(grid()?.shifted(seconds)),
            Regrid::BeatAt(position_s) => Some(grid()?.with_beat_at(position_s)),
            Regrid::Reset => None,
        };
        Ok(())
    })
}

/// How far to move the grid of the track on `deck` onto the beats of the deck it is mixed with,
/// both playing on a grid.
fn shift_to_other(state: &AppState, deck: usize) -> AppResult<Option<f64>> {
    let status = state.engine_status()?;
    let Some(bpm) = state.with_track(deck, |t| t.grid().map(|g| g.bpm))? else { return Ok(None) };
    let other = mixed_with(&status, deck);
    Ok(grid_shift_to(&status.decks[deck], &status.decks[other], bpm))
}

/// Sets the key from its Camelot code; `None` goes back to the detected key.
#[tauri::command]
pub fn set_key(
    app: AppHandle,
    state: State<'_, AppState>,
    library: State<'_, Arc<LibraryService>>,
    deck: usize,
    camelot: Option<String>,
) -> AppResult<()> {
    let key = camelot.map(|code| Key::from_camelot(&code).ok_or(AppError::InvalidKey(code))).transpose()?;
    edit(&app, &state, &library, deck, |_, edits| {
        edits.key = key;
        Ok(())
    })
}

/// The 24 keys, in Camelot order.
#[tauri::command]
pub fn key_choices() -> Vec<KeyDto> {
    Key::all().map(KeyDto::from).collect()
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
        let (path, mut edits) = (track.path.clone(), track.edits);
        change(edits.apply(track.detected).tempo.map(GridEdit::from), &mut edits)?;
        let updated: Vec<_> = tracks
            .iter_mut()
            .enumerate()
            .filter_map(|(on, track)| {
                let track = track.as_mut().filter(|t| t.path == path)?;
                track.edits = edits;
                Some((on, track.load_id, track.grid(), TrackAnalysisDto::from(&*track)))
            })
            .collect();
        (path, edits, updated)
    };
    for (on, load_id, grid, dto) in updated {
        state.send_if_running(Command::SetBeatGrid { deck: on, grid })?;
        let update = DeckTrackUpdateDto { deck: on, load_id, analysis: Some(dto), stems: None };
        let _ = app.emit(DECK_TRACK_UPDATE_EVENT, update);
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
