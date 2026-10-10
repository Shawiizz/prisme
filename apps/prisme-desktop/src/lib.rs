//! Prisme desktop application: Tauri shell around the engine.
//!
//! * `audio_host`: runs the sound card stream and the engine on a dedicated thread;
//! * `state`: loaded tracks and deck settings, re-applied when the audio restarts;
//! * `commands`: what the frontend can ask;
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
mod state;
mod status_stream;

use analysis_service::AnalysisService;
use state::AppState;
use std::sync::Arc;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|app| {
            let library = Arc::new(AnalysisService::open(app.handle(), library::music_root()));
            app.manage(library.clone());

            // Start with the default devices; the user can change them in the settings.
            let state = app.state::<AppState>();
            dev_startup::preload_tracks(&state, &library);
            let settings = dto::AudioSettings { simulate: dev_startup::simulate_requested(), ..Default::default() };
            if let Err(e) = commands::start_audio(&state, settings) {
                eprintln!("audio could not start: {e}");
            }
            if let Some(gain) = dev_startup::master_gain() {
                let _ = state.send(prisme_engine::Command::SetMasterGain { gain });
            }
            // analyse new and changed files of the music folder, in the background
            library.start();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_devices,
            commands::timecode_formats,
            commands::get_audio_settings,
            commands::audio_info,
            commands::apply_audio_settings,
            commands::load_track,
            commands::loaded_tracks,
            commands::mixer_state,
            commands::set_playing,
            commands::seek,
            commands::set_pitch,
            commands::set_volume,
            commands::set_vinyl,
            commands::set_crossfader,
            commands::set_sync_locked,
            commands::sync_once,
            commands::set_sync_leader,
            commands::music_root,
            commands::list_dir,
            commands::library_progress,
            commands::rescan_library,
            status_stream::subscribe_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Prisme");
}
