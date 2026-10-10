//! Prisme desktop application: Tauri shell around the engine.
//!
//! * `audio_host`: runs the sound card stream and the engine on a dedicated thread;
//! * `state`: preferences, loaded tracks and deck settings;
//! * `preferences`: what is kept from one start to the next;
//! * `commands`: what the frontend can ask, by domain;
//! * `status_stream`: engine state pushed to the frontend at ~60 Hz;
//! * `scope_stream`: a turntable's signal pushed while its oscilloscope is shown;
//! * `folders`: browsing music folders;
//! * `library_service`: track database (analysis, cues) and background analysis;
//! * `dto`: JSON shapes shared with the frontend.

mod audio_host;
mod commands;
mod dev_startup;
mod dto;
mod error;
mod folders;
mod library_service;
mod preferences;
mod scope_stream;
mod state;
mod status_stream;

use library_service::LibraryService;
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
            let library =
                Arc::new(LibraryService::open(app.handle(), data_dir.as_deref(), folders::music_root()));
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
            commands::audio::audio_backends,
            commands::audio::list_devices,
            commands::audio::timecode_formats,
            commands::audio::get_audio_settings,
            commands::audio::audio_info,
            commands::audio::apply_audio_settings,
            commands::tracks::load_track,
            commands::tracks::loaded_tracks,
            commands::decks::set_playing,
            commands::decks::seek,
            commands::decks::set_pitch,
            commands::decks::set_vinyl,
            commands::decks::set_vinyl_swap,
            commands::decks::set_sync_locked,
            commands::mixer::mixer_state,
            commands::mixer::set_mix_mode,
            commands::mixer::set_volume,
            commands::mixer::set_crossfader,
            commands::mixer::set_deck_cue,
            commands::mixer::set_cue_mix,
            commands::mixer::set_headphone_gain,
            commands::decks::sync_once,
            commands::decks::set_sync_leader,
            commands::transport::set_pitch_bend,
            commands::transport::beat_loop,
            commands::transport::loop_roll,
            commands::transport::exit_loop,
            commands::transport::reloop,
            commands::transport::scale_loop,
            commands::transport::beat_jump,
            commands::transport::set_slip,
            commands::transport::censor,
            commands::cues::set_hot_cue,
            commands::cues::trigger_hot_cue,
            commands::cues::delete_hot_cue,
            commands::cues::set_main_cue,
            commands::folders::music_root,
            commands::folders::list_dir,
            commands::library::library_progress,
            commands::library::rescan_library,
            commands::preferences::get_preferences,
            commands::preferences::set_dj_preferences,
            commands::preferences::set_library_preferences,
            status_stream::subscribe_status,
            scope_stream::subscribe_scope,
            scope_stream::unsubscribe_scope,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Prisme");
}

fn open_preferences(data_dir: Option<&Path>) -> JsonFile<Preferences> {
    let Some(dir) = data_dir else {
        return JsonFile::in_memory(Preferences::default());
    };
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
    if let Err(e) = commands::audio::start_audio(state, &settings) {
        eprintln!("saved audio settings unusable ({e}), using the default devices");
        let defaults = dto::AudioSettings { simulate: settings.simulate, ..Default::default() };
        if let Err(e) = commands::audio::start_audio(state, &defaults) {
            eprintln!("audio could not start: {e}");
        }
    }
}
