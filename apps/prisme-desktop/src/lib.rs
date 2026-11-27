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
//! * `clock`: the view built and followed about 60 times a second; `status_stream`: what
//!   changed of it told to the frontend;
//! * `scope_stream`: a turntable's signal pushed while its oscilloscope is shown;
//! * `track_loading`: putting a track on a deck, playable before it is fully decoded;
//! * `preview`: a track of the library heard in the headphones before it goes on a deck;
//! * `folders`: browsing music folders;
//! * `file_drop`: the paths of files dropped from the file manager (Windows and macOS);
//! * `picked`: the files picked in the system's dialogs, a path or Android's URI;
//! * `library_service`: track database (analysis, tags, cues) and background analysis;
//! * `cover_protocol`: cover thumbnails served to the UI;
//! * `dto`: JSON shapes shared with the frontend.

mod actions;
#[cfg(target_os = "android")]
mod android_jni;
mod audio_host;
mod audio_watchdog;
mod auto_dj;
mod choices;
mod clock;
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
#[cfg(any(target_os = "android", test))]
mod lock_player;
mod logging;
mod midi_clock;
#[cfg(target_os = "android")]
mod music_access;
mod picked;
mod preferences;
mod preview;
mod recording;
mod sampler;
mod scope_stream;
mod shortcuts;
mod state;
mod status_stream;
mod stems_service;
mod tag_writer;
mod track_loading;
mod track_waveform;
#[cfg(test)]
mod ts_types;
#[cfg(desktop)]
mod updates;
#[cfg(target_os = "android")]
mod usb_android;

use actions::Actions;
use controllers::Controllers;
use crash_recovery::{CrashRecovery, SavedDeck};
use dto::AudioSettings;
use error::AppError;
use library_service::LibraryService;
use midi_clock::MidiClock;
use prisme_audio_io::DeviceKind;
use prisme_session::JsonFile;
use serde::de::DeserializeOwned;
use serde::Serialize;
use state::{lock, AppState, Section};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tag_writer::TagWriter;
use tauri::Manager;
use tracing::{error, warn};

/// How long quitting waits for the actions sent before to be done.
const CLOSE_WAIT: Duration = Duration::from_secs(2);

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
/// would share the library), and Prisme updates itself from the releases (`updates`).
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
    builder.manage(updates::Updates::new())
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
    #[cfg(desktop)]
    if let Some(code) = updates::before_start() {
        std::process::exit(code);
    }
    let builder = tauri::Builder::default();
    #[cfg(desktop)]
    let builder = desktop_plugins(builder);
    builder
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol(cover_protocol::SCHEME, cover_protocol::handle)
        .setup(|app| {
            // a development start on data of its own logs beside them
            let logs =
                dev_startup::data_dir().map(|d| d.join("logs")).or_else(|| app.path().app_log_dir().ok());
            app.manage(logging::init(logs.as_deref()));
            prepare_window(app);
            let data_dir = data_dir(app);
            let library = manage_states(app, data_dir.as_deref());
            let restored = restore_decks(app, &library);
            #[cfg(not(target_os = "android"))]
            start_audio(app.handle(), &restored);
            start_services(app, &library);
            #[cfg(desktop)]
            commands::screens::start(app.handle());
            #[cfg(target_os = "android")]
            start_audio_apart(app.handle().clone(), restored);
            Ok(())
        })
        .on_window_event(window_event)
        .invoke_handler(invoke_handler())
        .build(tauri::generate_context!())
        .expect("error while building Prisme")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                close(app);
                #[cfg(desktop)]
                app.state::<updates::Updates>().on_quit();
            }
        });
}

/// The main window closed: the decks' with it, and Prisme quits. The decks' closed by the DJ:
/// the decks back in the main window.
fn window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    #[cfg(desktop)]
    match (window.label(), event) {
        ("main", tauri::WindowEvent::CloseRequested { .. }) => {
            if let Some(decks) = window.app_handle().get_webview_window(commands::screens::DECKS) {
                let _ = decks.destroy();
            }
        }
        (commands::screens::DECKS, tauri::WindowEvent::CloseRequested { .. }) => {
            commands::screens::closed_by_the_dj(window.app_handle());
        }
        _ => {}
    }
    #[cfg(mobile)]
    let _ = (window, event);
}

/// Prisme's data folder: the one the environment asks for, else the system's.
fn data_dir<R: tauri::Runtime>(app: &impl Manager<R>) -> Option<PathBuf> {
    dev_startup::data_dir()
        .or_else(|| app.path().app_data_dir().inspect_err(|e| warn!("no data folder: {e}")).ok())
}

/// The window as the platform needs it, before anything shows in it. Linux needs nothing of
/// the app.
#[cfg_attr(target_os = "linux", allow(unused_variables))]
fn prepare_window(app: &tauri::App) {
    #[cfg(windows)]
    browser_keys_off(app);
    #[cfg(any(windows, target_os = "macos"))]
    file_drop::listen(app);
    #[cfg(target_os = "android")]
    commands::apk::clear_leftovers(app.handle());
    #[cfg(target_os = "ios")]
    fill_the_screen(app);
    if !keep_awake::run_at_full_speed() {
        warn!("the system may slow Prisme down when its window is behind others");
    }
}

/// The states the commands and the threads share, read from `data_dir`: the library's is
/// returned too, opened on the music folders.
fn manage_states(app: &tauri::App, data_dir: Option<&Path>) -> Arc<LibraryService> {
    app.manage(AppState::new(open_json(data_dir, "preferences.json")));
    app.manage(Actions::start());
    app.manage(commands::audio::ListedDevices::default());
    app.manage(status_stream::StatusStream::default());
    if let Some(data_dir) = data_dir {
        controller_profiles::install(data_dir);
    }
    app.manage(Controllers::new(open_json(data_dir, "controllers.json")));
    Controllers::keep_profiles_up_to_date(app.handle().clone());
    let (chosen, added) = app
        .state::<AppState>()
        .preferences
        .read(|p| (p.library.music_folder.clone(), p.library.folders.clone()));
    let root = folders::music_root(chosen.as_deref());
    // iOS lets Prisme read its own documents only: no folder is added next to them
    let added =
        if cfg!(target_os = "ios") { Vec::new() } else { added.into_iter().map(Into::into).collect() };
    let library = Arc::new(LibraryService::open(app.handle(), data_dir, root, added));
    app.manage(library.clone());
    app.manage(TagWriter::start(app.handle().clone()));
    app.manage(preview::Previews::default());
    let models = models_dir(app, data_dir);
    // a phone lets Prisme read the shared music, not write beside it
    #[cfg(mobile)]
    prisme_stems::track::store_under(models.with_file_name("stems"));
    app.manage(stems_service::StemsService::new(app.handle().clone(), models));
    app.state::<stems_service::StemsService>().show();
    stems_service::look_for_gpu(app.handle().clone());
    app.manage(CrashRecovery::new(open_json(data_dir, "session.json")));
    library
}

/// The decks as the last run left them if it crashed (else as the dev start asks), loaded:
/// those restored, to be put where they were once the audio runs.
fn restore_decks(app: &tauri::App, library: &LibraryService) -> Vec<Option<SavedDeck>> {
    let restored = app.state::<CrashRecovery>().decks_to_restore();
    #[cfg(target_os = "android")]
    usb_android::install();
    let state = app.state::<AppState>();
    dev_startup::preload_tracks(app.handle(), &state, library);
    CrashRecovery::preload(app.handle(), &state, library, &restored);
    restored
}

/// The audio started, the decks `restored` put where they were, then kept in case of a crash.
fn start_audio(app: &tauri::AppHandle, restored: &[Option<SavedDeck>]) {
    let state = app.state::<AppState>();
    start_saved_audio(&state);
    CrashRecovery::seek(&state, restored);
    CrashRecovery::start(app.clone());
    if let Some(gain) = dev_startup::master_gain() {
        if let Err(e) = state.send(prisme_engine::Command::SetMasterGain { gain }) {
            warn!("the master gain asked not set: {e}");
        }
    }
}

/// [`start_audio`] on a thread of its own, the setup not waiting for the sound card to open.
#[cfg(target_os = "android")]
fn start_audio_apart(app: tauri::AppHandle, restored: Vec<Option<SavedDeck>>) {
    std::thread::Builder::new()
        .name("prisme-audio-start".into())
        .spawn(move || start_audio(&app, &restored))
        .expect("cannot start the audio's start thread");
}

/// The threads that watch, play and tell: the audio's watchdog, the history, the Auto DJ, the
/// controllers and the MIDI clock, the clock of the view, the library's analysis.
fn start_services(app: &tauri::App, library: &Arc<LibraryService>) {
    let state = app.state::<AppState>();
    audio_watchdog::start(app.handle().clone());
    #[cfg(desktop)]
    keep_awake::while_playing(app.handle().clone());
    history::start(app.handle().clone());
    let auto_dj = state.preferences.read(|p| p.auto_dj);
    app.manage(auto_dj::AutoDj::new(app.handle().clone(), auto_dj));
    app.state::<auto_dj::AutoDj>().tell();
    auto_dj::AutoDj::start(app.handle().clone());
    sampler::load_saved(app.handle().clone());
    app.manage(MidiClock::new(app.state::<Controllers>().setup().clock_output));
    MidiClock::start(app.handle().clone());
    Controllers::watch(app.handle().clone());
    clock::start(app.handle().clone());
    app.state::<TagWriter>().wake();
    #[cfg(target_os = "android")]
    android_jni::install(app.handle());
    library.show_folders();
    library.browse();
    #[cfg(desktop)]
    updates::start(app.handle());
    library.start();
    library.watch();
}

/// The commands the page may invoke, by module.
fn invoke_handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        commands::audio::apply_audio_settings,
        commands::audio::audio_backends,
        commands::audio::change_audio_draft,
        commands::audio::decline_card,
        commands::audio::latency_channels,
        commands::audio::list_devices,
        commands::audio::measure_latency,
        commands::audio::timecode_formats,
        commands::auto_dj::set_auto_dj_fade,
        commands::controllers::cancel_learning,
        commands::controllers::choose_controller_profile,
        commands::controllers::decline_controller,
        commands::controllers::export_controller_profile,
        commands::controllers::forget_control,
        commands::controllers::learn_control,
        commands::controllers::look_for_controllers,
        commands::controllers::midi_outputs,
        commands::controllers::open_controller_profiles_folder,
        commands::controllers::reload_controller_profiles,
        commands::controllers::set_control_encoding,
        commands::controllers::set_controller_enabled,
        commands::controllers::set_midi_clock_output,
        commands::controllers::update_controller_profiles,
        commands::controllers::waiting_controllers,
        commands::crates::add_to_crate,
        commands::crates::create_crate,
        commands::crates::create_smart_crate,
        commands::crates::delete_crate,
        commands::crates::rename_crate,
        commands::crates::update_smart_crate,
        commands::decks::hand,
        commands::decks::jog,
        commands::decks::load_track,
        commands::decks::seek_deck,
        commands::decks::set_deck_mode,
        commands::decks::set_record,
        commands::decks::set_tempo_range,
        commands::decks::set_timecode,
        commands::decks::set_vinyl,
        commands::decks::set_vinyl_swap,
        commands::diagnostics::page_error,
        commands::diagnostics::save_diagnostics,
        commands::edits::edit_grid,
        commands::edits::key_choices,
        commands::edits::set_key,
        commands::edits::tap_tempo,
        commands::export::export_m3u,
        commands::export::export_rekordbox,
        commands::folders::add_music_folder,
        commands::folders::audio_extensions,
        commands::folders::list_dir,
        commands::folders::remove_music_folder,
        commands::folders::set_music_folder,
        commands::folders::show_in_file_manager,
        commands::folders::sort_dropped,
        commands::folders::storage_places,
        commands::history::delete_history_session,
        commands::history::export_history,
        commands::import::import_rekordbox,
        commands::import::import_serato,
        commands::import::serato_folder,
        commands::library::library_compare_with,
        commands::library::library_filter,
        commands::library::library_genres,
        commands::library::library_menu,
        commands::library::library_open,
        commands::library::library_scope,
        commands::library::library_select,
        commands::library::library_select_span,
        commands::library::library_sort,
        commands::library::library_up,
        commands::library::library_viewport,
        commands::library::remove_from_library,
        commands::library::remove_from_list,
        commands::library::rescan_library,
        commands::library::search_library,
        commands::library::separate_tracks,
        commands::live::set_live_input,
        commands::live::set_talkover_depth,
        commands::marks::rate_track,
        commands::marks::set_track_color,
        commands::marks::set_track_genre,
        commands::mixer::set_booth_output,
        commands::mixer::set_headphones_output,
        commands::mixer::set_limiter,
        #[cfg(mobile)]
        commands::new_version::check_update,
        commands::pads::anchor_slicer,
        commands::pads::pad_pages,
        commands::pads::show_fx_slot,
        commands::perform::let_go_key,
        commands::perform::perform_action,
        commands::preferences::get_preferences,
        commands::preferences::reset_keys,
        commands::preferences::set_keys,
        commands::preferences::set_preference,
        commands::prepare::add_to_prepare,
        commands::prepare::place_in_list,
        commands::preview::preview_waveform,
        commands::preview::seek_preview,
        commands::preview::toggle_preview,
        commands::recording::show_recording,
        commands::recording::start_recording,
        commands::recording::stop_recording,
        commands::sampler::load_sample,
        commands::sampler::take_sample_off,
        commands::screens::list_screens,
        commands::screens::decks_apart,
        commands::screens::set_decks_screen,
        commands::screens::show_decks,
        commands::stems::cancel_separation,
        commands::stems::download_stems_model,
        commands::stems::download_stems_models,
        commands::stems::separate_library,
        commands::targets::list_targets,
        commands::tracks::stem_activity,
        commands::tracks::watch_waveform,
        scope_stream::subscribe_scope,
        scope_stream::unsubscribe_scope,
        status_stream::subscribe_status,
        #[cfg(debug_assertions)]
        commands::round_trip::ping,
        #[cfg(debug_assertions)]
        commands::simulated::simulate_hid_plug,
        #[cfg(debug_assertions)]
        commands::simulated::simulate_hid_report,
        #[cfg(debug_assertions)]
        commands::simulated::simulate_hid_set,
        #[cfg(debug_assertions)]
        commands::simulated::simulate_hid_unplug,
        #[cfg(debug_assertions)]
        commands::simulated::simulated_hid_written,
        #[cfg(desktop)]
        updates::update_state,
        #[cfg(desktop)]
        updates::look_for_update,
        #[cfg(desktop)]
        updates::download_update,
        #[cfg(desktop)]
        updates::restart_to_update,
        #[cfg(target_os = "android")]
        commands::apk::allow_installs,
        #[cfg(target_os = "android")]
        commands::apk::download_apk,
        #[cfg(target_os = "android")]
        commands::apk::install_apk,
    ]
}

/// What Prisme does as it quits: the actions sent before done, a while at most (a cue set
/// kept, a recording ended by a key closed), then a recording going on ended, its file closed
/// and its tracklist written, once one ending meanwhile is.
fn close(app: &tauri::AppHandle) {
    app.state::<Controllers>().close_all();
    if !app.state::<Actions>().finish(CLOSE_WAIT) {
        warn!("Prisme quits with actions not done");
    }
    let state = app.state::<AppState>();
    actions::general::stop_recording(&state);
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

/// The saved devices, or the default ones if they are missing (interface unplugged), or another
/// output if another application holds the default one; none if the audio runs already, the
/// DJ having chosen devices meanwhile.
fn start_saved_audio(state: &AppState) {
    let mut settings = state.preferences.get().audio;
    settings.simulate |= dev_startup::simulate_requested();
    let start = |settings: &AudioSettings| commands::audio::start_audio_unless_running(state, settings);
    let Err(e) = start(&settings) else { return };
    warn!("saved audio settings unusable ({e}), using the default devices");
    // the card chosen is there but does not open: another application holds it
    let held = settings.output.as_ref().filter(|_| audio_host::devices_available(&settings));
    if let Some(output) = held {
        *lock(&state.busy_output) = Some(audio_watchdog::card_of(&output.into()));
        state.moved(Section::Audio);
    }
    let defaults = settings.on_default_devices();
    let e = match start(&defaults) {
        Ok(_) => return,
        Err(AppError::Audio(e)) if e.device_busy() => e,
        Err(e) => return error!("audio could not start: {e}"),
    };
    let busy = prisme_audio_io::default_output();
    *lock(&state.busy_output) = busy.as_ref().map(audio_watchdog::card_of);
    state.moved(Section::Audio);
    let others = prisme_audio_io::list_devices(defaults.backend.into()).unwrap_or_default();
    let others = others
        .into_iter()
        .filter(|d| d.kind == DeviceKind::Output && busy.as_ref().is_none_or(|busy| busy.id != d.id));
    for output in others {
        let chosen = dto::DeviceChoice { id: output.id.clone(), name: output.name.clone() };
        let on_it = AudioSettings { output: Some(chosen), ..defaults.clone() };
        if start(&on_it).is_ok() {
            return warn!(
                "the default output is held by another application, the sound plays on {}",
                output.name
            );
        }
    }
    error!("audio could not start: {e}");
}
