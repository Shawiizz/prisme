use crate::commands::audio::start_audio;
use crate::commands::blocking;
use crate::error::AppResult;
use crate::library_service::LibraryService;
use crate::preferences::{DjPreferences, LibraryPreferences, Preferences};
use crate::state::{lock, AppState};
use crate::tag_writer::TagWriter;
use prisme_engine::Command;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

#[tauri::command]
pub fn get_preferences(state: State<'_, AppState>) -> Preferences {
    state.preferences.get()
}

/// Off the main thread: another number of decks restarts the audio.
#[tauri::command]
pub async fn set_dj_preferences(app: AppHandle, dj: DjPreferences) -> AppResult<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        let decks_before = state.preferences.read(|p| p.dj.decks());
        state.update_preferences(|p| p.dj = dj);
        // another number of decks is another engine: the audio starts again, the tracks stay
        // on the decks there still are
        lock(&state.tracks)[dj.decks()..].fill_with(|| None);
        let running = lock(&state.session).as_ref().map(|s| s.settings.clone());
        if let Some(settings) = running.filter(|_| dj.decks() != decks_before) {
            start_audio(&state, &settings)?;
        }
        state.send_if_running(Command::SetQuantize { enabled: dj.quantize })?;
        state.apply_levels()
    })
    .await
}

/// Off the main thread: every track of the library may be marked to be tagged again.
#[tauri::command]
pub async fn set_library_preferences(app: AppHandle, library: LibraryPreferences) -> AppResult<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        let (write_tags, replace_tags) = (library.write_tags, library.replace_tags);
        let replaced_before = state.preferences.read(|p| p.library.replace_tags);
        state.update_preferences(|p| p.library = library);
        // the tempos and keys of other programs left until now are to be written over
        if replace_tags && !replaced_before {
            app.state::<Arc<LibraryService>>().db.retag_all()?;
        }
        if write_tags {
            app.state::<TagWriter>().wake();
        }
        Ok(())
    })
    .await
}
