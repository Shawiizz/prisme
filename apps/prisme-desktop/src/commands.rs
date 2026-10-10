//! Commands callable from the frontend (`invoke('name', args)`).

use crate::audio_host;
use crate::dto::{
    AudioInfo, AudioSettings, DeckSettingsDto, DeviceDto, DirListingDto, MixerStateDto, ProgressDto,
    TimecodeFormatDto, TrackAnalysisDto, TrackInfoDto, VinylModeDto,
};
use crate::analysis_service::AnalysisService;
use crate::error::{AppError, AppResult};
use crate::library;
use crate::state::{deck_commands, lock, AppState, LoadedTrack};
use prisme_analysis::AnalysisOptions;
use prisme_audio_io::DeviceKind;
use prisme_library::{FileStamp, TrackRecord};
use prisme_engine::Command;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

/// Waveform resolution: enough for a zoomed, scrolling view.
pub const WAVEFORM_SLICES_PER_S: u32 = 150;

#[tauri::command]
pub fn list_devices() -> AppResult<Vec<DeviceDto>> {
    Ok(prisme_audio_io::list_devices()?
        .into_iter()
        .map(|d| DeviceDto {
            kind: match d.kind {
                DeviceKind::Input => "input",
                DeviceKind::Output => "output",
            },
            name: d.name,
            channels: d.channels,
            sample_rate: d.sample_rate,
            is_default: d.is_default,
        })
        .collect())
}

#[tauri::command]
pub fn timecode_formats() -> Vec<TimecodeFormatDto> {
    prisme_timecode::FORMATS.iter().map(|f| TimecodeFormatDto { id: f.id, name: f.name }).collect()
}

#[tauri::command]
pub fn get_audio_settings(state: State<'_, AppState>) -> AudioSettings {
    lock(&state.settings).clone()
}

#[tauri::command]
pub fn audio_info(state: State<'_, AppState>) -> Option<AudioInfo> {
    lock(&state.session).as_ref().map(|s| s.info.clone())
}

/// (Re)starts the audio with new settings and restores the decks.
#[tauri::command]
pub fn apply_audio_settings(state: State<'_, AppState>, settings: AudioSettings) -> AppResult<AudioInfo> {
    start_audio(&state, settings)
}

pub fn start_audio(state: &AppState, settings: AudioSettings) -> AppResult<AudioInfo> {
    // stop the current session first: the device may not open twice
    lock(&state.session).take();
    if settings.simulate {
        // the simulated needle starts 10 s into the record: start the track there
        let mut decks = lock(&state.decks);
        decks[0].vinyl_enabled = true;
        decks[0].lead_in_s = prisme_timecode::testing::SimulatedTurntable::START_S;
    }
    let mut session = audio_host::start(&settings)?;
    state.restore(&mut session.controller);
    let info = session.info.clone();
    *lock(&state.session) = Some(session);
    *lock(&state.settings) = settings;
    Ok(info)
}

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

/// Deck settings and crossfader as the backend knows them (to rebuild the UI).
#[tauri::command]
pub fn mixer_state(state: State<'_, AppState>) -> MixerStateDto {
    MixerStateDto {
        decks: lock(&state.decks)
            .iter()
            .map(|d| DeckSettingsDto {
                pitch: d.pitch,
                volume: d.volume,
                vinyl_enabled: d.vinyl_enabled,
                vinyl_mode: d.vinyl_mode,
                lead_in_s: d.lead_in_s,
            })
            .collect(),
        crossfader: *lock(&state.crossfader),
    }
}

#[tauri::command]
pub fn set_playing(state: State<'_, AppState>, deck: usize, playing: bool) -> AppResult<()> {
    AppState::check_deck(deck)?;
    state.send(Command::SetPlaying { deck, playing })
}

#[tauri::command]
pub fn seek(state: State<'_, AppState>, deck: usize, seconds: f64) -> AppResult<()> {
    AppState::check_deck(deck)?;
    state.send(Command::Seek { deck, seconds })
}

#[tauri::command]
pub fn set_pitch(state: State<'_, AppState>, deck: usize, ratio: f64) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].pitch = ratio;
    state.send(Command::SetPitch { deck, ratio })
}

#[tauri::command]
pub fn set_volume(state: State<'_, AppState>, deck: usize, gain: f32) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].volume = gain;
    state.send(Command::SetVolume { deck, gain })
}

#[tauri::command]
pub fn set_vinyl(
    state: State<'_, AppState>,
    deck: usize,
    enabled: bool,
    mode: VinylModeDto,
    lead_in_s: f64,
) -> AppResult<()> {
    AppState::check_deck(deck)?;
    let settings = {
        let mut decks = lock(&state.decks);
        let d = &mut decks[deck];
        d.vinyl_enabled = enabled;
        d.vinyl_mode = mode;
        d.lead_in_s = lead_in_s;
        *d
    };
    for c in deck_commands(deck, &settings) {
        state.send(c)?;
    }
    Ok(())
}

#[tauri::command]
pub fn set_crossfader(state: State<'_, AppState>, position: f32) -> AppResult<()> {
    *lock(&state.crossfader) = position;
    state.send(Command::SetCrossfader { position })
}

#[tauri::command]
pub fn set_sync_locked(state: State<'_, AppState>, deck: usize, locked: bool) -> AppResult<()> {
    AppState::check_deck(deck)?;
    state.send(Command::SetSyncLocked { deck, locked })
}

#[tauri::command]
pub fn sync_once(state: State<'_, AppState>, deck: usize) -> AppResult<()> {
    AppState::check_deck(deck)?;
    state.send(Command::SyncOnce { deck })
}

/// `None`: the leader is picked automatically.
#[tauri::command]
pub fn set_sync_leader(state: State<'_, AppState>, deck: Option<usize>) -> AppResult<()> {
    if let Some(d) = deck {
        AppState::check_deck(d)?;
    }
    state.send(Command::SetSyncLeader { deck })
}

#[tauri::command]
pub fn music_root() -> String {
    library::music_root().display().to_string()
}

/// Folders and tracks of a folder, with the stored analysis of each track.
#[tauri::command]
pub fn list_dir(library: State<'_, Arc<AnalysisService>>, path: String) -> AppResult<DirListingDto> {
    Ok(library::list_dir(&PathBuf::from(path), |file, meta| {
        let record = library.db.get(file).ok().flatten()?;
        record.is_current(FileStamp::from_metadata(meta)).then(|| TrackAnalysisDto::from(&record))
    })?)
}

/// State of the background analysis of the music folder.
#[tauri::command]
pub fn library_progress(library: State<'_, Arc<AnalysisService>>) -> ProgressDto {
    library.progress().into()
}

/// Looks for new or changed files in the music folder and analyses them.
#[tauri::command]
pub fn rescan_library(library: State<'_, Arc<AnalysisService>>) {
    library.start();
}
