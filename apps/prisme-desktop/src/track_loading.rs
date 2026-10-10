//! Loading a track on a deck, in two steps: first what makes it playable at once (the
//! start of the audio, what the library knows of it, its cached waveform); then, on a
//! worker thread, the rest of the audio and whatever was missing (waveform, analysis),
//! sent to the UI as it comes (`deck-track-update`).

use crate::dto::{DeckTrackUpdateDto, TrackAnalysisDto};
use crate::error::AppResult;
use crate::library_service::LibraryService;
use crate::state::{lock, AppState, LoadedTrack};
use prisme_analysis::{AnalysisOptions, TrackAnalysis};
use prisme_engine::Command;
use prisme_library::{FileStamp, TrackRecord};
use prisme_media::LoadingTrack;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tracing::{error, warn};

pub const DECK_TRACK_UPDATE_EVENT: &str = "deck-track-update";
/// Waveform resolution: enough for a zoomed, scrolling view.
pub const WAVEFORM_SLICES_PER_S: u32 = 150;

/// What is left to do once the track is on the deck.
pub struct Completion {
    loading: LoadingTrack,
    path: PathBuf,
    stamp: Option<FileStamp>,
    waveform_missing: bool,
    analysis_missing: bool,
}

/// The track, playable, and what is left to do ([`Completion::run`]).
/// Puts a track on `deck` before the audio starts (`AppState::restore` hands it to the
/// engine); its waveform and analysis follow.
pub fn preload(
    app: &AppHandle,
    state: &AppState,
    library: &LibraryService,
    deck: usize,
    path: PathBuf,
) -> AppResult<()> {
    let (track, completion) = prepare(path, library)?;
    let load_id = track.load_id;
    lock(&state.tracks)[deck] = Some(track);
    let app = app.clone();
    std::thread::spawn(move || completion.run(&app, deck, load_id));
    Ok(())
}

pub fn prepare(path: PathBuf, library: &LibraryService) -> AppResult<(LoadedTrack, Completion)> {
    let loading = LoadingTrack::open(&path)?;
    let stamp = FileStamp::of(&path).ok();
    let stored = stamp
        .and_then(|s| library.db.get(&path).ok().flatten().filter(|r| r.is_current(s) && r.error.is_none()));
    let waveform = stamp.and_then(|s| library.db.waveform(&path, s).ok().flatten());
    let completion = Completion {
        path: path.clone(),
        stamp,
        waveform_missing: waveform.is_none(),
        analysis_missing: stored.is_none(),
        loading,
    };
    let (detected, edits, tags) = match stored {
        Some(r) => (r.analysis, r.edits, r.tags),
        None => (
            TrackAnalysis::default(),
            library.db.edits(&path).unwrap_or_default(),
            prisme_media::read_tags(&path).map(|(tags, _)| tags).unwrap_or_default(),
        ),
    };
    let cues = library.db.cues(&path).unwrap_or_else(|e| {
        warn!("cues of {} not read: {e}", path.display());
        Vec::new()
    });
    let track = LoadedTrack {
        load_id: LoadedTrack::next_load_id(),
        audio: completion.loading.audio().clone(),
        path,
        waveform,
        detected,
        edits,
        tags,
        cues,
        key_shift: 0,
    };
    Ok((track, completion))
}

impl Completion {
    /// Runs on a worker thread. The results go to the track loaded on `deck` as
    /// `load_id`, and nowhere if another one was loaded meanwhile.
    pub fn run(self, app: &AppHandle, deck: usize, load_id: u64) {
        let (audio, decoded) = self.loading.decode_rest();
        if let Err(e) = decoded {
            warn!("{} decoded up to {:.1} s: {e}", self.path.display(), audio.duration_s());
        }
        let state = app.state::<AppState>();
        let library = app.state::<Arc<LibraryService>>();
        let mut update = DeckTrackUpdateDto { deck, load_id, waveform_ready: false, analysis: None };
        if self.waveform_missing {
            let waveform = prisme_analysis::compute_waveform(&audio, WAVEFORM_SLICES_PER_S);
            if let Some(stamp) = self.stamp {
                if let Err(e) = library.db.save_waveform(&self.path, stamp, &waveform) {
                    error!("waveform of {} not saved: {e}", self.path.display());
                }
            }
            update.waveform_ready =
                state.with_loaded(deck, load_id, |t| t.waveform = Some(waveform)).is_some();
        }
        if self.analysis_missing {
            let analysis = prisme_analysis::analyze(&audio, &AnalysisOptions::default());
            let metadata = prisme_library::read_metadata(&self.path);
            if let Some(stamp) = self.stamp {
                let analyzed =
                    TrackRecord::analyzed(&self.path, stamp, audio.duration_s(), analysis, metadata.tags);
                let edits = library.db.edits(&self.path).unwrap_or_default();
                library.store(&TrackRecord { edits, ..analyzed }, metadata.cover.as_deref());
            }
            let applied = state.with_loaded(deck, load_id, |t| {
                t.detected = analysis;
                (t.grid(), TrackAnalysisDto::from(&*t))
            });
            if let Some((grid, dto)) = applied {
                let _ = state.send_if_running(Command::SetBeatGrid { deck, grid });
                update.analysis = Some(dto);
            }
        }
        if update.waveform_ready || update.analysis.is_some() {
            let _ = app.emit(DECK_TRACK_UPDATE_EVENT, update);
        }
    }
}
