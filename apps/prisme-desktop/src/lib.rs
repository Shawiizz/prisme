//! Prisme desktop application: Tauri shell around the engine.
//!
//! * `audio_host`: runs the sound card stream and the engine on a dedicated thread;
//! * `audio_watchdog`: restarts the audio when its device comes back after a loss;
//! * `controllers`: MIDI controllers and their mappings;
//! * `crash_recovery`: puts the decks back after a crash;
//! * `history`: records the tracks played, set by set;
//! * `logging`: the log files;
//! * `midi_clock`: MIDI clock out, in time with the sync leader;
//! * `recording`: records the mix to a file, with its tracklist;
//! * `sampler`: the decks' sampler sounds;
//! * `state`: preferences, loaded tracks and deck settings;
//! * `tag_writer`: writes the tempo and key into the files' tags;
//! * `preferences`: what is kept from one start to the next;
//! * `commands`: what the frontend can ask, by domain;
//! * `status_stream`: engine state pushed to the frontend at ~60 Hz;
//! * `scope_stream`: a turntable's signal pushed while its oscilloscope is shown;
//! * `track_loading`: putting a track on a deck, playable before it is fully decoded;
//! * `folders`: browsing music folders;
//! * `library_service`: track database (analysis, tags, cues) and background analysis;
//! * `cover_protocol`: cover thumbnails served to the UI;
//! * `dto`: JSON shapes shared with the frontend.

mod audio_host;
mod audio_watchdog;
mod commands;
mod controllers;
mod cover_protocol;
mod crash_recovery;
mod dev_startup;
mod dto;
mod error;
mod folder_watch;
mod folders;
mod history;
mod library_service;
mod logging;
mod midi_clock;
mod preferences;
mod recording;
mod sampler;
mod scope_stream;
mod state;
mod status_stream;
mod tag_writer;
mod track_loading;

use controllers::Controllers;
use crash_recovery::CrashRecovery;
use library_service::LibraryService;
use midi_clock::MidiClock;
use prisme_session::JsonFile;
use serde::de::DeserializeOwned;
use serde::Serialize;
use state::AppState;
use std::path::Path;
use std::sync::Arc;
use tag_writer::TagWriter;
use tauri::Manager;
use tracing::{error, warn};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .register_asynchronous_uri_scheme_protocol(cover_protocol::SCHEME, cover_protocol::handle)
        .setup(|app| {
            app.manage(logging::init(app.path().app_log_dir().ok().as_deref()));
            let data_dir = dev_startup::data_dir()
                .or_else(|| app.path().app_data_dir().inspect_err(|e| warn!("no data folder: {e}")).ok());
            app.manage(AppState::new(open_json(data_dir.as_deref(), "preferences.json")));
            app.manage(Controllers::new(open_json(data_dir.as_deref(), "controllers.json")));
            let library =
                Arc::new(LibraryService::open(app.handle(), data_dir.as_deref(), folders::music_root()));
            app.manage(library.clone());
            app.manage(TagWriter::start(app.handle().clone()));

            app.manage(CrashRecovery::new(open_json(data_dir.as_deref(), "session.json")));
            let restored = app.state::<CrashRecovery>().decks_to_restore();

            let state = app.state::<AppState>();
            dev_startup::preload_tracks(app.handle(), &state, &library);
            CrashRecovery::preload(app.handle(), &state, &library, &restored);
            start_saved_audio(&state);
            CrashRecovery::seek(&state, &restored);
            CrashRecovery::start(app.handle().clone());
            if let Some(gain) = dev_startup::master_gain() {
                let _ = state.send(prisme_engine::Command::SetMasterGain { gain });
            }
            audio_watchdog::start(app.handle().clone());
            history::start(app.handle().clone());
            sampler::load_saved(app.handle().clone());
            let controllers = app.state::<Controllers>();
            controllers.open_enabled(app.handle());
            app.manage(MidiClock::new());
            if let Some(output) = controllers.setup().clock_output {
                if let Err(e) = app.state::<MidiClock>().set_output(Some(&output)) {
                    warn!("MIDI clock not sent to {output}: {e}");
                }
            }
            MidiClock::start(app.handle().clone());
            app.state::<TagWriter>().wake();
            library.start();
            library.watch();
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
            commands::tracks::deck_waveform,
            commands::decks::set_playing,
            commands::decks::seek,
            commands::decks::set_pitch,
            commands::decks::set_vinyl,
            commands::decks::set_vinyl_swap,
            commands::decks::set_key_lock,
            commands::decks::set_key_shift,
            commands::fx::set_fx,
            commands::fx::set_fx_enabled,
            commands::fx::set_fx_mix,
            commands::fx::set_fx_amount,
            commands::fx::set_fx_beats,
            commands::decks::set_sync_locked,
            commands::mixer::mixer_state,
            commands::mixer::set_mix_mode,
            commands::mixer::set_volume,
            commands::mixer::set_crossfader,
            commands::mixer::set_deck_cue,
            commands::mixer::set_cue_mix,
            commands::mixer::set_headphone_gain,
            commands::mixer::set_trim,
            commands::mixer::set_eq,
            commands::mixer::set_eq_kill,
            commands::mixer::set_filter,
            commands::mixer::set_crossfader_curve,
            commands::mixer::set_limiter,
            commands::decks::sync_once,
            commands::decks::set_sync_leader,
            commands::transport::set_pitch_bend,
            commands::transport::beat_loop,
            commands::transport::loop_roll,
            commands::transport::loop_in,
            commands::transport::loop_out,
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
            commands::edits::edit_grid,
            commands::edits::set_key,
            commands::edits::key_choices,
            commands::folders::music_root,
            commands::folders::list_dir,
            commands::library::library_progress,
            commands::library::rescan_library,
            commands::library::search_library,
            commands::library::find_duplicates,
            commands::crates::list_crates,
            commands::crates::create_crate,
            commands::crates::create_smart_crate,
            commands::crates::set_crate_rules,
            commands::crates::rename_crate,
            commands::crates::delete_crate,
            commands::crates::add_to_crate,
            commands::crates::remove_from_crate,
            commands::crates::crate_tracks,
            commands::history::history_sessions,
            commands::history::history_plays,
            commands::history::delete_history_session,
            commands::history::export_history,
            commands::marks::set_track_color,
            commands::marks::set_track_rating,
            commands::import::serato_folder,
            commands::import::import_serato,
            commands::import::import_rekordbox,
            commands::sampler::load_sample,
            commands::sampler::clear_sample,
            commands::sampler::trigger_sample,
            commands::sampler::stop_sample,
            commands::sampler::set_sampler_gain,
            commands::controllers::list_controllers,
            commands::controllers::set_controller_enabled,
            commands::controllers::controller_mappings,
            commands::controllers::learn_control,
            commands::controllers::cancel_learning,
            commands::controllers::forget_control,
            commands::controllers::set_lights,
            commands::diagnostics::save_diagnostics,
            commands::controllers::midi_outputs,
            commands::controllers::midi_clock_output,
            commands::controllers::set_midi_clock_output,
            commands::recording::start_recording,
            commands::recording::stop_recording,
            commands::prepare::prepare_list,
            commands::prepare::add_to_prepare,
            commands::prepare::remove_from_prepare,
            commands::prepare::move_in_prepare,
            commands::preferences::get_preferences,
            commands::preferences::set_dj_preferences,
            commands::preferences::set_library_preferences,
            status_stream::subscribe_status,
            scope_stream::subscribe_scope,
            scope_stream::unsubscribe_scope,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Prisme")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                app.state::<CrashRecovery>().closed_normally();
            }
        });
}

fn open_json<T: Serialize + DeserializeOwned + Default + Clone>(
    data_dir: Option<&Path>,
    name: &str,
) -> JsonFile<T> {
    let Some(dir) = data_dir else {
        return JsonFile::in_memory(T::default());
    };
    let (file, set_aside) = JsonFile::open(dir.join(name));
    if let Some(path) = set_aside {
        warn!("unreadable {name} moved to {}", path.display());
    }
    file
}

/// The saved devices, or the default ones if they are missing (interface unplugged).
fn start_saved_audio(state: &AppState) {
    let mut settings = state.preferences.get().audio;
    settings.simulate |= dev_startup::simulate_requested();
    if let Err(e) = commands::audio::start_audio(state, &settings) {
        warn!("saved audio settings unusable ({e}), using the default devices");
        let defaults = dto::AudioSettings { simulate: settings.simulate, ..Default::default() };
        if let Err(e) = commands::audio::start_audio(state, &defaults) {
            error!("audio could not start: {e}");
        }
    }
}
