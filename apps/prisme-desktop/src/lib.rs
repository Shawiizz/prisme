//! Prisme desktop application: Tauri shell around the engine.
//!
//! * `audio_host`: runs the sound card stream and the engine on a dedicated thread;
//! * `state`: loaded tracks and deck settings, re-applied when the audio restarts;
//! * `commands`: what the frontend can ask;
//! * `status_stream`: engine state pushed to the frontend at ~60 Hz;
//! * `library`: browsing music folders;
//! * `dto`: JSON shapes shared with the frontend.

mod audio_host;
mod commands;
mod dev_startup;
mod dto;
mod error;
mod library;
mod state;
mod status_stream;

use state::AppState;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|app| {
            // Start with the default devices; the user can change them in the settings.
            let state = app.state::<AppState>();
            dev_startup::preload_tracks(&state);
            let settings = dto::AudioSettings { simulate: dev_startup::simulate_requested(), ..Default::default() };
            if let Err(e) = commands::start_audio(&state, settings) {
                eprintln!("audio could not start: {e}");
            }
            if let Some(gain) = dev_startup::master_gain() {
                let _ = state.send(prisme_engine::Command::SetMasterGain { gain });
            }
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
            commands::music_root,
            commands::list_dir,
            status_stream::subscribe_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Prisme");
}
