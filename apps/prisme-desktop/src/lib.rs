//! Prisme desktop application: Tauri shell around the engine.
//!
//! * `actions`: what the controllers and the interface ask for, performed, and what their
//!   lights read;
//! * `auto_dj`: plays the prepare list, each track mixed into the next;
//! * `audio_host`: runs the sound card stream and the engine on a dedicated thread;
//! * `audio_watchdog`: restarts the audio when its device comes back after a loss;
//! * `controllers`: MIDI and HID controllers and their mappings;
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
//! * `preview`: a track of the library heard in the headphones before it goes on a deck;
//! * `folders`: browsing music folders;
//! * `file_drop`: the paths of files dropped from the file manager (Windows and macOS);
//! * `library_service`: track database (analysis, tags, cues) and background analysis;
//! * `cover_protocol`: cover thumbnails served to the UI;
//! * `dto`: JSON shapes shared with the frontend.

mod actions;
#[cfg(target_os = "android")]
mod android_jni;
mod audio_host;
mod audio_watchdog;
mod auto_dj;
mod commands;
mod controller_hid;
mod controller_profiles;
mod controllers;
mod cover_protocol;
mod crash_recovery;
mod dev_startup;
mod dto;
mod error;
#[cfg(any(windows, target_os = "macos"))]
mod file_drop;
mod folder_watch;
mod folders;
mod history;
mod http;
mod input_access;
mod keep_awake;
mod library_service;
mod logging;
mod midi_clock;
#[cfg(target_os = "android")]
mod music_access;
mod preferences;
mod preview;
mod recording;
mod sampler;
mod scope_stream;
mod state;
mod status_stream;
mod stems_service;
mod tag_writer;
mod track_loading;
mod track_waveform;
#[cfg(target_os = "android")]
mod usb_android;

use controllers::Controllers;
use crash_recovery::CrashRecovery;
use library_service::LibraryService;
use midi_clock::MidiClock;
use prisme_session::JsonFile;
use serde::de::DeserializeOwned;
use serde::Serialize;
use state::{lock, AppState};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tag_writer::TagWriter;
use tauri::Manager;
use tracing::{error, warn};

/// The window maximised where its first size does not fit the screen (a laptop at 150 %).
#[cfg(desktop)]
fn fit_window(app: &tauri::App) {
    let Some(window) = app.get_webview_window("main") else { return };
    let (Ok(Some(monitor)), Ok(size)) = (window.current_monitor(), window.outer_size()) else { return };
    let area = monitor.work_area().size;
    if size.width > area.width || size.height > area.height {
        let _ = window.maximize();
    }
}

/// The keys of the browser under the page off in a release (reload, print, find, zoom, its
/// developer tools): Prisme is an application, its keys are its own. Kept in development.
#[cfg(windows)]
fn browser_keys_off(app: &tauri::App) {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
    use windows_core::Interface;

    if cfg!(debug_assertions) {
        return;
    }
    let Some(window) = app.get_webview_window("main") else { return };
    let done = window.with_webview(|webview| {
        // SAFETY: the controller of the window's WebView2, alive during this call on the main
        // thread; the settings are its own, changed through their COM interface
        let off = unsafe {
            webview
                .controller()
                .CoreWebView2()
                .and_then(|core| core.Settings())
                .and_then(|settings| settings.cast::<ICoreWebView2Settings3>())
                .and_then(|settings| settings.SetAreBrowserAcceleratorKeysEnabled(false))
        };
        if let Err(e) = off {
            warn!("the browser's keys stay on: {e}");
        }
    });
    if let Err(e) = done {
        warn!("the browser's keys stay on: {e}");
    }
}

/// Where the separation models go: with the data in development, else in the local data
/// folder, kept out of roaming profiles (they weigh 1.4 GB). Models a version before put
/// with the roaming data move there.
fn models_dir(app: &tauri::App, data_dir: Option<&Path>) -> PathBuf {
    let data = data_dir.unwrap_or(Path::new(".")).join("models");
    match (dev_startup::data_dir(), app.path().app_local_data_dir()) {
        (None, Ok(local)) => folders::moved(&data, local.join("models")),
        _ => data,
    }
}

/// What only a computer has: a second launch on the same data shows the running one (two
/// would share the library), and updates come from the releases, not from a store.
#[cfg(desktop)]
fn desktop_plugins(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    let builder = if dev_startup::data_dir().is_none() {
        builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
    } else {
        builder
    };
    builder
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(commands::update::Downloaded::default())
}

/// The page over the whole screen of an iPhone, under the notch and the home bar (it keeps
/// clear of them itself): left alone, the web view counts them out of its height and leaves
/// a band empty at the bottom.
#[cfg(target_os = "ios")]
fn fill_the_screen(app: &tauri::App) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    /// `UIScrollViewContentInsetAdjustmentNever`
    const NEVER: isize = 2;

    let Some(window) = app.get_webview_window("main") else { return };
    let filled = window.with_webview(|webview| {
        let webview = webview.inner().cast::<AnyObject>();
        // SAFETY: `inner` is the WKWebView of the window, alive during this call on the main
        // thread; both messages are UIKit's, with the types it declares.
        unsafe {
            let scroll: *mut AnyObject = msg_send![webview, scrollView];
            let () = msg_send![scroll, setContentInsetAdjustmentBehavior: NEVER];
        }
    });
    if let Err(e) = filled {
        warn!("the page keeps out of the notch and the home bar: {e}");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(desktop)]
    let builder = desktop_plugins(builder);
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .register_asynchronous_uri_scheme_protocol(cover_protocol::SCHEME, cover_protocol::handle)
        .setup(|app| {
            app.manage(logging::init(app.path().app_log_dir().ok().as_deref()));
            #[cfg(desktop)]
            fit_window(app);
            #[cfg(windows)]
            browser_keys_off(app);
            #[cfg(any(windows, target_os = "macos"))]
            file_drop::listen(app);
            #[cfg(windows)]
            commands::update::clear_leftovers(app.handle());
            #[cfg(target_os = "android")]
            commands::apk::clear_leftovers(app.handle());
            #[cfg(target_os = "ios")]
            fill_the_screen(app);
            if !keep_awake::run_at_full_speed() {
                warn!("the system may slow Prisme down when its window is behind others");
            }
            let data_dir = dev_startup::data_dir()
                .or_else(|| app.path().app_data_dir().inspect_err(|e| warn!("no data folder: {e}")).ok());
            app.manage(AppState::new(open_json(data_dir.as_deref(), "preferences.json")));
            app.manage(actions::Actions::start());
            if let Some(data_dir) = data_dir.as_deref() {
                controller_profiles::install(data_dir);
            }
            app.manage(Controllers::new(open_json(data_dir.as_deref(), "controllers.json")));
            Controllers::keep_profiles_up_to_date(app.handle().clone());
            let (chosen, added) = app
                .state::<AppState>()
                .preferences
                .read(|p| (p.library.music_folder.clone(), p.library.folders.clone()));
            let root = folders::music_root(chosen.as_deref());
            // iOS lets Prisme read its own documents only: no folder is added next to them
            let added = if cfg!(target_os = "ios") {
                Vec::new()
            } else {
                added.into_iter().map(Into::into).collect()
            };
            let library = Arc::new(LibraryService::open(app.handle(), data_dir.as_deref(), root, added));
            app.manage(library.clone());
            app.manage(TagWriter::start(app.handle().clone()));
            app.manage(preview::Previews::default());
            let models = models_dir(app, data_dir.as_deref());
            // a phone lets Prisme read the shared music, not write beside it
            #[cfg(mobile)]
            prisme_stems::track::store_under(models.with_file_name("stems"));
            app.manage(stems_service::StemsService::new(app.handle().clone(), models));

            app.manage(CrashRecovery::new(open_json(data_dir.as_deref(), "session.json")));
            let restored = app.state::<CrashRecovery>().decks_to_restore();

            #[cfg(target_os = "android")]
            usb_android::install();
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
            #[cfg(desktop)]
            keep_awake::while_playing(app.handle().clone());
            history::start(app.handle().clone());
            let auto_dj = state.preferences.read(|p| p.auto_dj);
            app.manage(auto_dj::AutoDj::new(app.handle().clone(), auto_dj));
            auto_dj::AutoDj::start(app.handle().clone());
            sampler::load_saved(app.handle().clone());
            let controllers = app.state::<Controllers>();
            controllers.open_at_start(app.handle());
            Controllers::watch(app.handle().clone());
            Controllers::drive_outputs(app.handle().clone());
            app.manage(MidiClock::new(controllers.setup().clock_output));
            MidiClock::start(app.handle().clone());
            app.state::<TagWriter>().wake();
            #[cfg(target_os = "android")]
            music_access::install(app.handle());
            library.start();
            library.watch();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::audio::audio_backends,
            commands::audio::set_hidden,
            commands::audio::list_devices,
            commands::audio::timecode_formats,
            commands::audio::get_audio_settings,
            commands::audio::audio_info,
            commands::audio::apply_audio_settings,
            commands::audio::measure_latency,
            commands::stems::stems_status,
            commands::stems::stems_gpu,
            commands::screens::screen_below,
            commands::screens::span_screens,
            commands::stems::download_stems_model,
            commands::stems::separate_tracks,
            commands::stems::separate_library,
            commands::stems::cancel_separation,
            commands::tracks::loaded_tracks,
            commands::preview::preview_track,
            commands::preview::stop_preview,
            commands::preview::seek_preview,
            commands::preview::skip_preview,
            commands::preview::preview_waveform,
            commands::tracks::watch_waveform,
            commands::tracks::stem_activity,
            commands::decks::seek,
            commands::decks::hand,
            commands::decks::jog,
            commands::decks::set_pitch,
            commands::decks::set_vinyl,
            commands::decks::set_vinyl_swap,
            commands::decks::set_timecode,
            commands::decks::set_tempo_range,
            commands::fx::set_fx_mix,
            commands::fx::set_fx_amount,
            commands::fx::set_fx_param2,
            commands::fx::set_fx_chain_enabled,
            commands::mixer::mixer_state,
            commands::mixer::set_mix_mode,
            commands::mixer::set_headphones_output,
            commands::mixer::set_volume,
            commands::mixer::set_crossfader_reverse,
            commands::mixer::set_crossfader,
            commands::mixer::set_deck_cue,
            commands::mixer::set_cue_mix,
            commands::mixer::set_split_cue,
            commands::mixer::set_headphone_gain,
            commands::mixer::set_trim,
            commands::mixer::set_eq,
            commands::mixer::set_eq_kill,
            commands::mixer::set_filter,
            commands::mixer::set_crossfader_curve,
            commands::mixer::set_limiter,
            commands::mixer::set_balance,
            commands::mixer::set_main_gain,
            commands::mixer::set_crossfader_side,
            commands::mixer::set_booth_output,
            commands::mixer::set_booth_gain,
            commands::live::set_live_input,
            commands::live::set_live_level,
            commands::live::set_live_on,
            commands::live::set_live_cue,
            commands::live::set_talkover,
            commands::live::set_talkover_depth,
            commands::transport::beat_loop,
            commands::transport::loop_in,
            commands::transport::loop_out,
            commands::transport::anchor_slicer,
            commands::transport::exit_loop,
            commands::transport::reloop,
            commands::transport::scale_loop,
            commands::transport::beat_jump,
            commands::transport::set_slip,
            commands::transport::censor,
            commands::transport::brake,
            commands::transport::spin_back,
            commands::transport::set_reverse,
            commands::edits::edit_grid,
            commands::edits::set_key,
            commands::edits::key_choices,
            commands::folders::music_root,
            commands::folders::set_music_folder,
            commands::folders::music_folders,
            commands::folders::add_music_folder,
            commands::folders::remove_music_folder,
            commands::folders::show_in_file_manager,
            commands::folders::storage_places,
            commands::recording::show_recording,
            commands::folders::list_dir,
            commands::library::library_progress,
            commands::library::library_in_memory,
            commands::library::rescan_library,
            commands::library::search_library,
            commands::library::find_duplicates,
            commands::library::elsewhere_tracks,
            commands::library::remove_tracks,
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
            commands::marks::set_track_genre,
            commands::library::library_genres,
            commands::import::serato_folder,
            commands::import::import_serato,
            commands::import::import_rekordbox,
            commands::export::export_m3u,
            commands::export::export_rekordbox,
            commands::sampler::load_sample,
            commands::controllers::list_controllers,
            commands::controllers::set_controller_enabled,
            commands::controllers::choose_controller_profile,
            commands::controllers::decline_controller,
            commands::controllers::waiting_controllers,
            commands::audio::decline_card,
            commands::controllers::controller_mappings,
            commands::controllers::controller_light_targets,
            commands::controllers::learn_control,
            commands::controllers::set_control_encoding,
            commands::controllers::cancel_learning,
            commands::controllers::forget_control,
            commands::controllers::set_lights,
            actions::perform_action,
            actions::loads::set_library_selection,
            commands::controllers::controller_profiles,
            commands::controllers::reload_controller_profiles,
            commands::controllers::update_controller_profiles,
            commands::controllers::export_controller_profile,
            commands::controllers::open_controller_profiles_folder,
            commands::diagnostics::save_diagnostics,
            #[cfg(debug_assertions)]
            commands::simulated::simulate_hid_plug,
            #[cfg(debug_assertions)]
            commands::simulated::simulate_hid_unplug,
            #[cfg(debug_assertions)]
            commands::simulated::simulate_hid_report,
            #[cfg(debug_assertions)]
            commands::simulated::simulate_hid_set,
            #[cfg(debug_assertions)]
            commands::simulated::simulated_hid_written,
            commands::controllers::midi_outputs,
            commands::controllers::midi_clock_output,
            commands::controllers::set_midi_clock_output,
            commands::recording::start_recording,
            commands::recording::stop_recording,
            commands::prepare::prepare_list,
            commands::prepare::add_to_prepare,
            commands::prepare::remove_from_prepare,
            commands::prepare::move_in_prepare,
            commands::auto_dj::auto_dj_status,
            commands::auto_dj::toggle_auto_dj,
            commands::auto_dj::auto_dj_fade_now,
            commands::auto_dj::auto_dj_skip,
            commands::auto_dj::set_auto_dj_fade,
            commands::preferences::get_preferences,
            commands::preferences::set_dj_preferences,
            commands::preferences::set_library_preferences,
            commands::preferences::set_key_preferences,
            #[cfg(desktop)]
            commands::update::download_update,
            #[cfg(desktop)]
            commands::update::install_update,
            #[cfg(target_os = "android")]
            commands::apk::download_apk,
            #[cfg(target_os = "android")]
            commands::apk::install_apk,
            #[cfg(target_os = "android")]
            commands::apk::allow_installs,
            status_stream::subscribe_status,
            scope_stream::subscribe_scope,
            scope_stream::unsubscribe_scope,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Prisme")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                close(app);
            }
        });
}

/// What Prisme does as it quits.
fn close(app: &tauri::AppHandle) {
    // a recording going on is ended: its file closed, its tracklist written
    let state = app.state::<AppState>();
    let recorder = lock(&state.recorder).take();
    if let Some(recorder) = recorder {
        recorder.stop(&state);
    }
    app.state::<Controllers>().close_all();
    state.preferences.flush();
    app.state::<CrashRecovery>().closed_normally();
}

fn open_json<T: Serialize + DeserializeOwned + Default + Clone + Send + 'static>(
    data_dir: Option<&Path>,
    name: &str,
) -> JsonFile<T> {
    let Some(dir) = data_dir else {
        return JsonFile::in_memory(T::default());
    };
    let (file, opened) = JsonFile::open(dir.join(name));
    if let Some(path) = opened.set_aside {
        warn!("unreadable {name} moved to {}", path.display());
    }
    if opened.held {
        error!("{name} could not be read: this session's changes to it are not saved");
    }
    file
}

/// The saved devices, or the default ones if they are missing (interface unplugged).
fn start_saved_audio(state: &AppState) {
    let mut settings = state.preferences.get().audio;
    settings.simulate |= dev_startup::simulate_requested();
    if let Err(e) = commands::audio::start_audio(state, &settings) {
        warn!("saved audio settings unusable ({e}), using the default devices");
        if let Err(e) = commands::audio::start_audio(state, &settings.on_default_devices()) {
            error!("audio could not start: {e}");
        }
    }
}
