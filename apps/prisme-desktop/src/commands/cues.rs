use crate::dto::CueDto;
use crate::error::{AppError, AppResult};
use crate::library_service::LibraryService;
use crate::state::AppState;
use prisme_engine::{BeatGrid, Command, LoopRegion};
use prisme_library::{Cue, CueSlot, HOT_CUES};
use std::sync::Arc;
use tauri::State;

/// Sets hot cue `slot` where the deck is (on the nearest beat with quantize on), or as
/// a saved loop if a loop is playing. Returns all the cues of the track.
#[tauri::command]
pub fn set_hot_cue(
    state: State<'_, AppState>,
    library: State<'_, Arc<LibraryService>>,
    deck: usize,
    slot: u8,
) -> AppResult<Vec<CueDto>> {
    let slot = hot_slot(slot)?;
    let status = state.deck_status(deck)?;
    let quantize = state.preferences.get().dj.quantize;
    state.with_track(deck, |track| {
        let cue = match status.looping.region.filter(|_| status.looping.active) {
            Some(region) => Cue::looped(slot, region.start_s, region.length_s()),
            None => Cue::point(slot, snapped(track.grid(), status.position_s, quantize)),
        };
        library.db.save_cue(&track.path, &cue)?;
        track.put_cue(cue);
        Ok(track.cue_dtos())
    })?
}

/// Jumps to hot cue `slot`; a saved loop starts looping.
#[tauri::command]
pub fn trigger_hot_cue(state: State<'_, AppState>, deck: usize, slot: u8) -> AppResult<()> {
    let slot = hot_slot(slot)?;
    let Some(cue) = state.with_track(deck, |track| track.cue(slot).cloned())? else { return Ok(()) };
    if let Some(length_s) = cue.loop_length_s {
        let region = LoopRegion { start_s: cue.position_s, end_s: cue.position_s + length_s };
        state.send(Command::SetLoop { deck, region: Some(region), active: true })?;
    }
    state.send(Command::Jump { deck, seconds: cue.position_s })
}

#[tauri::command]
pub fn delete_hot_cue(
    state: State<'_, AppState>,
    library: State<'_, Arc<LibraryService>>,
    deck: usize,
    slot: u8,
) -> AppResult<Vec<CueDto>> {
    let slot = hot_slot(slot)?;
    state.with_track(deck, |track| {
        library.db.delete_cue(&track.path, slot)?;
        track.remove_cue(slot);
        Ok(track.cue_dtos())
    })?
}

/// Stores the main cue (on the nearest beat with quantize on); returns where it went.
#[tauri::command]
pub fn set_main_cue(
    state: State<'_, AppState>,
    library: State<'_, Arc<LibraryService>>,
    deck: usize,
    seconds: f64,
) -> AppResult<f64> {
    let quantize = state.preferences.get().dj.quantize;
    state.with_track(deck, |track| {
        let cue = Cue::point(CueSlot::Main, snapped(track.grid(), seconds, quantize));
        library.db.save_cue(&track.path, &cue)?;
        let position = cue.position_s;
        track.put_cue(cue);
        Ok(position)
    })?
}

fn hot_slot(slot: u8) -> AppResult<CueSlot> {
    if slot < HOT_CUES {
        Ok(CueSlot::Hot(slot))
    } else {
        Err(AppError::InvalidCue(slot))
    }
}

fn snapped(grid: Option<BeatGrid>, position_s: f64, quantize: bool) -> f64 {
    match grid {
        Some(grid) if quantize => grid.nearest_beat_s(position_s),
        _ => position_s,
    }
}
