use crate::analysis_service::AnalysisService;
use crate::dto::TrackInfoDto;
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState, LoadedTrack};
use prisme_analysis::AnalysisOptions;
use prisme_library::{FileStamp, TrackRecord};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

/// Waveform resolution: enough for a zoomed, scrolling view.
pub const WAVEFORM_SLICES_PER_S: u32 = 150;

/// Decodes a file, analyses it (waveform, tempo, key) and loads it on a deck.
#[tauri::command]
pub async fn load_track(
    state: State<'_, AppState>,
    library: State<'_, Arc<AnalysisService>>,
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
/// analysis comes from the library when it is up to date there; otherwise it is
/// computed and stored.
pub fn prepare_track(path: PathBuf, library: &AnalysisService) -> AppResult<LoadedTrack> {
    let audio = prisme_media::decode_file(&path)?;
    let stamp = FileStamp::of(&path).ok();
    let stored = stamp
        .and_then(|s| library.db.get(&path).ok().flatten().filter(|r| r.is_current(s) && r.error.is_none()))
        .map(|r| r.analysis);
    let (waveform, analysis) = std::thread::scope(|s| {
        let waveform = s.spawn(|| prisme_analysis::compute_waveform(&audio, WAVEFORM_SLICES_PER_S));
        let analysis = stored.unwrap_or_else(|| {
            let analysis = prisme_analysis::analyze(&audio, &AnalysisOptions::default());
            if let Some(stamp) = stamp {
                let record = TrackRecord::analyzed(&path, stamp, audio.duration_s(), analysis);
                if let Err(e) = library.db.save(&record) {
                    eprintln!("{e}");
                }
                library.announce(&record);
            }
            analysis
        });
        (waveform.join().expect("waveform thread"), analysis)
    });
    Ok(LoadedTrack { path, audio: Arc::new(audio), waveform, analysis })
}

/// Tracks currently loaded on the decks (to rebuild the UI after a reload).
#[tauri::command]
pub fn loaded_tracks(state: State<'_, AppState>) -> Vec<Option<TrackInfoDto>> {
    lock(&state.tracks).iter().enumerate().map(|(deck, t)| t.as_ref().map(|t| t.info(deck))).collect()
}
