use crate::commands::audio::start_audio;
use crate::commands::blocking;
use crate::error::AppResult;
use crate::library_service::LibraryService;
use crate::preferences::{DjPreferences, KeyPreference, LibraryPreferences, Preferences};
use crate::state::{lock, AppState};
use crate::stems_service::StemsService;
use crate::tag_writer::TagWriter;
use std::collections::BTreeMap;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

#[tauri::command]
pub fn get_preferences(state: State<'_, AppState>) -> Preferences {
    state.preferences.get()
}

/// Off the main thread: another number of decks restarts the audio. The quantize and the tempo
/// range stay as the actions set them (`general.quantize`, `set_tempo_range`): a window showing
/// older ones does not take a controller's change back.
#[tauri::command]
pub async fn set_dj_preferences(app: AppHandle, dj: DjPreferences) -> AppResult<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        let decks_before = state.preferences.read(|p| p.dj.decks());
        state.update_preferences(|p| p.dj = dj.keeping_actions_of(&p.dj));
        // another number of decks is another engine: the audio starts again, the tracks stay
        // on the decks there still are
        lock(&state.tracks)[dj.decks()..].fill_with(|| None);
        let running = lock(&state.session).as_ref().map(|s| s.settings.clone());
        if let Some(settings) = running.filter(|_| dj.decks() != decks_before) {
            start_audio(&state, &settings)?;
        }
        state.apply_levels()
    })
    .await
}

/// Off the main thread: every track of the library may be marked to be tagged again. The
/// music folders stay: they change with `set_music_folder`, `add_music_folder` and
/// `remove_music_folder`. Other separation models chosen take over the track being separated.
#[tauri::command]
pub async fn set_library_preferences(app: AppHandle, mut library: LibraryPreferences) -> AppResult<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        let (write_tags, replace_tags, fast) = (library.write_tags, library.replace_tags, library.stems_fast);
        let (replaced_before, fast_before) =
            state.preferences.read(|p| (p.library.replace_tags, p.library.stems_fast));
        state.update_preferences(|p| {
            library.music_folder = p.library.music_folder.take();
            library.folders = std::mem::take(&mut p.library.folders);
            p.library = library;
        });
        // the tempos and keys of other programs left until now are to be written over
        if replace_tags && !replaced_before {
            app.state::<Arc<LibraryService>>().db.retag_all()?;
        }
        if write_tags {
            app.state::<TagWriter>().wake();
        }
        if fast != fast_before {
            app.state::<StemsService>().rechosen();
        }
        Ok(())
    })
    .await
}

/// The keyboard keys the DJ changed, by action.
#[tauri::command]
pub fn set_key_preferences(state: State<'_, AppState>, keys: BTreeMap<String, Vec<KeyPreference>>) {
    state.update_preferences(|p| p.keys = keys);
}
