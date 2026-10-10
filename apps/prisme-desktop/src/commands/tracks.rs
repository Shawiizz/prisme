use crate::dto::TrackInfoDto;
use crate::error::{AppError, AppResult};
use crate::library_service::LibraryService;
use crate::state::{lock, AppState, LoadedTrack};
use prisme_analysis::{AnalysisOptions, TrackAnalysis};
use prisme_library::{FileStamp, TrackRecord};
use prisme_media::{TrackAudio, TrackTags};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::State;

/// Waveform resolution: enough for a zoomed, scrolling view.
pub const WAVEFORM_SLICES_PER_S: u32 = 150;

/// Decodes a file, analyses it (waveform, tempo, key) and loads it on a deck.
#[tauri::command]
pub async fn load_track(
    state: State<'_, AppState>,
    library: State<'_, Arc<LibraryService>>,
    deck: usize,
    path: String,
) -> AppResult<TrackInfoDto> {
    AppState::check_deck(deck)?;
    let file = PathBuf::from(&path);
    let library = library.inner().clone();
    let track = tauri::async_runtime::spawn_blocking(move || prepare_track(file, &library))
        .await
        .map_err(|e| AppError::Other(e.to_string()))??;
    let info = track.info(deck);
    let commands = track.load_commands(deck);
    // kept for audio restarts
    lock(&state.tracks)[deck] = Some(track);
    for c in commands {
        state.send_if_running(c)?;
    }
    Ok(info)
}

/// Decoding, then the waveform and the tempo / key analysis side by side. The
/// analysis and tags come from the library when they are up to date there; otherwise
/// they are computed and stored.
pub fn prepare_track(path: PathBuf, library: &LibraryService) -> AppResult<LoadedTrack> {
    let audio = prisme_media::decode_file(&path)?;
    let stamp = FileStamp::of(&path).ok();
    let stored = stamp
        .and_then(|s| library.db.get(&path).ok().flatten().filter(|r| r.is_current(s) && r.error.is_none()));
    let (waveform, (analysis, tags)) = std::thread::scope(|s| {
        let waveform = s.spawn(|| prisme_analysis::compute_waveform(&audio, WAVEFORM_SLICES_PER_S));
        let known = match stored {
            Some(r) => (r.analysis, r.tags),
            None => analyze_and_store(&path, stamp, &audio, library),
        };
        (waveform.join().expect("waveform thread"), known)
    });
    let cues = library.db.cues(&path).unwrap_or_else(|e| {
        eprintln!("cues of {} not read: {e}", path.display());
        Vec::new()
    });
    Ok(LoadedTrack {
        load_id: LoadedTrack::next_load_id(),
        path,
        audio: Arc::new(audio),
        waveform,
        analysis,
        tags,
        cues,
    })
}

fn analyze_and_store(
    path: &Path,
    stamp: Option<FileStamp>,
    audio: &TrackAudio,
    library: &LibraryService,
) -> (TrackAnalysis, TrackTags) {
    let analysis = prisme_analysis::analyze(audio, &AnalysisOptions::default());
    let metadata = prisme_library::read_metadata(path);
    let Some(stamp) = stamp else { return (analysis, metadata.tags) };
    let record = TrackRecord::analyzed(path, stamp, audio.duration_s(), analysis, metadata.tags);
    library.store(&record, metadata.cover.as_deref());
    (record.analysis, record.tags)
}

/// Tracks currently loaded on the decks (to rebuild the UI after a reload).
#[tauri::command]
pub fn loaded_tracks(state: State<'_, AppState>) -> Vec<Option<TrackInfoDto>> {
    lock(&state.tracks).iter().enumerate().map(|(deck, t)| t.as_ref().map(|t| t.info(deck))).collect()
}
