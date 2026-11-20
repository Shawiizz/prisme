//! Corrections of a loaded track's beat grid and key: applied at once to every deck playing the
//! track and to the engine, then stored in the library in turn (see `Actions::keep`) and shown
//! in its lists; the decks playing the track show them as they are told (`deck-track-update`).

use crate::actions::Actions;
use crate::dto::{DeckTrackUpdateDto, KeyDto, TrackAnalysisDto};
use crate::error::{AppError, AppResult};
use crate::library_service::LibraryService;
use crate::state::{lock, AppState};
use crate::track_loading::DECK_TRACK_UPDATE_EVENT;
use prisme_analysis::{Key, KeyEstimate};
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
    /// Back to the analysis.
    Reset,
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
    let position_s = match change {
        GridChange::BeatHere => state.deck_status(deck)?.position_s,
        _ => 0.0,
    };
    edit(app, state, library, deck, |track_grid, edits| {
        let grid = || track_grid.ok_or(AppError::NoBeatGrid(deck));
        let with_bpm = |g: GridEdit, bpm: f64| g.with_bpm(bpm).ok_or(AppError::InvalidBpm(bpm));
        edits.grid = match change {
            GridChange::Bpm { bpm } => {
                Some(with_bpm(track_grid.unwrap_or(GridEdit { bpm, first_beat_s: 0.0 }), bpm)?)
            }
            GridChange::Scale { factor } => Some(with_bpm(grid()?, grid()?.bpm * factor)?),
            GridChange::Shift { seconds } => Some(grid()?.shifted(seconds)),
            GridChange::BeatHere => Some(grid()?.with_beat_at(position_s)),
            GridChange::Reset => None,
        };
        Ok(())
    })
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
    Key::all().map(|key| KeyDto::from(&KeyEstimate { key, confidence: 1.0, tuning_cents: 0.0 })).collect()
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
    AppState::check_deck(deck)?;
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
