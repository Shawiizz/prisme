use crate::auto_dj::{AutoDj, Takeover};
use crate::dto::{CueDto, DeckTrackUpdateDto};
use crate::error::{AppError, AppResult};
use crate::library_service::LibraryService;
use crate::state::{AppState, LoadedTrack};
use crate::track_loading::DECK_TRACK_UPDATE_EVENT;
use prisme_engine::{BeatGrid, Command, LoopRegion};
use prisme_library::{Cue, CueSlot, HOT_CUES};
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

/// Sets hot cue `slot` where the deck is (on the nearest beat with quantize on), or as
/// a saved loop if a loop is playing. Returns all the cues of the track.
#[tauri::command]
pub fn set_hot_cue(
    app: AppHandle,
    state: State<'_, AppState>,
    library: State<'_, Arc<LibraryService>>,
    deck: usize,
    slot: u8,
) -> AppResult<Vec<CueDto>> {
    let slot = hot_slot(slot)?;
    let status = state.deck_status(deck)?;
    let quantize = state.preferences.get().dj.quantize;
    let (path, cue) = state.with_track(deck, |track| {
        let cue = match status.looping.region.filter(|_| status.looping.active) {
            Some(region) => Cue::looped(slot, region.start_s, region.length_s()),
            None => Cue::point(slot, snapped(track.grid(), status.position_s, quantize)),
        };
        (track.path.clone(), cue)
    })?;
    library.db.save_cue(&path, &cue)?;
    change_cues(&app, &state, deck, &path, |track| track.put_cue(cue.clone()))
}

/// Jumps to hot cue `slot`; a saved loop starts looping.
#[tauri::command]
pub fn trigger_hot_cue(
    state: State<'_, AppState>,
    auto_dj: State<'_, AutoDj>,
    deck: usize,
    slot: u8,
) -> AppResult<()> {
    let slot = hot_slot(slot)?;
    auto_dj.take_over(Takeover::Transport(deck));
    let Some(cue) = state.with_track(deck, |track| track.cue(slot).cloned())? else { return Ok(()) };
    if let Some(length_s) = cue.loop_length_s {
        let region = LoopRegion { start_s: cue.position_s, end_s: cue.position_s + length_s };
        state.send(Command::SetLoop { deck, region: Some(region), active: true })?;
    }
    state.send(Command::Jump { deck, seconds: cue.position_s })
}

#[tauri::command]
pub fn delete_hot_cue(
    app: AppHandle,
    state: State<'_, AppState>,
    library: State<'_, Arc<LibraryService>>,
    deck: usize,
    slot: u8,
) -> AppResult<Vec<CueDto>> {
    let slot = hot_slot(slot)?;
    let path = state.with_track(deck, |track| track.path.clone())?;
    library.db.delete_cue(&path, slot)?;
    change_cues(&app, &state, deck, &path, |track| track.remove_cue(slot))
}

/// Stores the main cue (on the nearest beat with quantize on); returns where it went.
#[tauri::command]
pub fn set_main_cue(
    app: AppHandle,
    state: State<'_, AppState>,
    library: State<'_, Arc<LibraryService>>,
    deck: usize,
    seconds: f64,
) -> AppResult<f64> {
    let quantize = state.preferences.get().dj.quantize;
    let (path, cue) = state.with_track(deck, |track| {
        (track.path.clone(), Cue::point(CueSlot::Main, snapped(track.grid(), seconds, quantize)))
    })?;
    library.db.save_cue(&path, &cue)?;
    let position = cue.position_s;
    change_cues(&app, &state, deck, &path, |track| track.put_cue(cue.clone()))?;
    Ok(position)
}

/// Applies `change` to the cues of `path` on every deck playing it and tells the UI those of
/// the other decks; returns those of `deck`.
fn change_cues(
    app: &AppHandle,
    state: &AppState,
    deck: usize,
    path: &Path,
    change: impl FnMut(&mut LoadedTrack),
) -> AppResult<Vec<CueDto>> {
    let (cues, others) = cue_updates(state, deck, path, change);
    for update in others {
        let _ = app.emit(DECK_TRACK_UPDATE_EVENT, update);
    }
    cues.ok_or(AppError::NoTrack(deck))
}

/// Applies `change` on every deck playing `path`: the cues of `deck`, if it plays it, and
/// the updates for the others.
fn cue_updates(
    state: &AppState,
    deck: usize,
    path: &Path,
    mut change: impl FnMut(&mut LoadedTrack),
) -> (Option<Vec<CueDto>>, Vec<DeckTrackUpdateDto>) {
    let mut cues = None;
    let mut others = Vec::new();
    let changed = state.with_path(path, |track| {
        change(track);
        track.cue_dtos()
    });
    for (on, load_id, dtos) in changed {
        if on == deck {
            cues = Some(dtos);
        } else {
            others.push(DeckTrackUpdateDto {
                deck: on,
                load_id,
                analysis: None,
                separation: None,
                cues: Some(dtos),
            });
        }
    }
    (cues, others)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::lock;
    use crate::track_waveform::TrackWaveform;
    use prisme_media::TrackAudio;
    use prisme_session::JsonFile;
    use std::path::PathBuf;

    fn track(path: &str) -> LoadedTrack {
        LoadedTrack {
            load_id: LoadedTrack::next_load_id(),
            path: PathBuf::from(path),
            audio: Arc::new(TrackAudio::from_interleaved(48_000, vec![0.0; 2])),
            waveform: TrackWaveform::growing(1),
            detected: Default::default(),
            edits: Default::default(),
            tags: Default::default(),
            cues: Vec::new(),
            key_shift: 0,
            stems: Default::default(),
            stem_mix: Default::default(),
            loudness: None,
        }
    }

    fn hot(cues: &[CueDto]) -> Vec<Option<u8>> {
        cues.iter().map(|c| c.hot).collect()
    }

    #[test]
    fn a_cue_changes_on_every_deck_playing_the_file() {
        let state = AppState::new(JsonFile::in_memory(Default::default()));
        let a = track("drop.flac");
        let doubled = a.double();
        let (doubled_load, other) = (doubled.load_id, track("other.flac"));
        *lock(&state.tracks) = [Some(a), Some(doubled), Some(other), None];
        let path = Path::new("drop.flac");

        let (cues, others) = cue_updates(&state, 0, path, |t| t.put_cue(Cue::point(CueSlot::Hot(0), 30.0)));
        assert_eq!(hot(&cues.unwrap()), [Some(0)]);
        assert_eq!(others.len(), 1, "the deck of another file keeps its cues");
        assert_eq!((others[0].deck, others[0].load_id), (1, doubled_load));
        assert_eq!(hot(others[0].cues.as_ref().unwrap()), [Some(0)]);
        assert!(state.with_track(1, |t| t.cue(CueSlot::Hot(0)).is_some()).unwrap());
        assert!(state.with_track(2, |t| t.cues.is_empty()).unwrap());

        let (cues, others) = cue_updates(&state, 1, path, |t| t.remove_cue(CueSlot::Hot(0)));
        assert!(cues.unwrap().is_empty());
        assert_eq!(others[0].deck, 0);
        assert!(state.with_track(0, |t| t.cues.is_empty()).unwrap());
    }
}
