//! Prisme desktop application: Tauri shell around the engine.
//!
//! * `audio_host`: runs the sound card stream and the engine on a dedicated thread;
//! * `state`: preferences, loaded tracks and deck settings;
//! * `preferences`: what is kept from one start to the next;
//! * `commands`: what the frontend can ask, by domain;
//! * `status_stream`: engine state pushed to the frontend at ~60 Hz;
//! * `library`: browsing music folders;
//! * `analysis_service`: track database and background analysis of the music folder;
//! * `dto`: JSON shapes shared with the frontend.

mod analysis_service;
mod audio_host;
mod commands;
mod dev_startup;
mod dto;
mod error;
mod library;
mod preferences;
mod state;
mod status_stream;

use analysis_service::AnalysisService;
use commands::{audio, decks, library as library_commands, preferences as preference_commands, tracks};
use preferences::Preferences;
use prisme_session::JsonFile;
use state::AppState;
use std::path::Path;
use std::sync::Arc;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir().inspect_err(|e| eprintln!("no data folder: {e}")).ok();
            app.manage(AppState::new(open_preferences(data_dir.as_deref())));
            let library = Arc::new(AnalysisService::open(app.handle(), data_dir.as_deref(), library::music_root()));
            app.manage(library.clone());

            let state = app.state::<AppState>();
            dev_startup::preload_tracks(&state, &library);
            start_saved_audio(&state);
            if let Some(gain) = dev_startup::master_gain() {
                let _ = state.send(prisme_engine::Command::SetMasterGain { gain });
            }
            library.start();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            audio::list_devices,
            audio::timecode_formats,
            audio::get_audio_settings,
            audio::audio_info,
            audio::apply_audio_settings,
            tracks::load_track,
            tracks::loaded_tracks,
            decks::mixer_state,
            decks::set_playing,
            decks::seek,
            decks::set_pitch,
            decks::set_volume,
            decks::set_vinyl,
            decks::set_crossfader,
            decks::set_sync_locked,
            decks::sync_once,
            decks::set_sync_leader,
            library_commands::music_root,
            library_commands::list_dir,
            library_commands::library_progress,
            library_commands::rescan_library,
            preference_commands::get_preferences,
            preference_commands::set_dj_preferences,
            preference_commands::set_library_preferences,
            status_stream::subscribe_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Prisme");
}

fn open_preferences(data_dir: Option<&Path>) -> JsonFile<Preferences> {
    let Some(dir) = data_dir else { return JsonFile::in_memory(Preferences::default()) };
    let (file, set_aside) = JsonFile::open(dir.join("preferences.json"));
    if let Some(path) = set_aside {
        eprintln!("unreadable preferences moved to {}", path.display());
    }
    file
}

/// The saved devices, or the default ones if they are missing (interface unplugged).
fn start_saved_audio(state: &AppState) {
    let mut settings = state.preferences.get().audio;
    settings.simulate |= dev_startup::simulate_requested();
    if let Err(e) = audio::start_audio(state, &settings) {
        eprintln!("saved audio settings unusable ({e}), using the default devices");
        let defaults = dto::AudioSettings { simulate: settings.simulate, ..Default::default() };
        if let Err(e) = audio::start_audio(state, &defaults) {
            eprintln!("audio could not start: {e}");
        }
    }
}
