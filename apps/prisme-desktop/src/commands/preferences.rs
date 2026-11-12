use crate::commands::audio::start_audio;
use crate::error::AppResult;
use crate::library_service::LibraryService;
use crate::preferences::{DjPreferences, LibraryPreferences, Preferences};
use crate::state::{lock, AppState};
use crate::tag_writer::TagWriter;
use prisme_engine::Command;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn get_preferences(state: State<'_, AppState>) -> Preferences {
    state.preferences.get()
}

#[tauri::command]
pub fn set_dj_preferences(state: State<'_, AppState>, dj: DjPreferences) -> AppResult<()> {
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
}

#[tauri::command]
pub fn set_library_preferences(
    state: State<'_, AppState>,
    tag_writer: State<'_, TagWriter>,
    service: State<'_, Arc<LibraryService>>,
    library: LibraryPreferences,
) -> AppResult<()> {
    let (write_tags, replace_tags) = (library.write_tags, library.replace_tags);
    let replaced_before = state.preferences.read(|p| p.library.replace_tags);
    state.update_preferences(|p| p.library = library);
    // the tempos and keys of other programs left until now are to be written over
    if replace_tags && !replaced_before {
        service.db.retag_all()?;
    }
    if write_tags {
        tag_writer.wake();
    }
    Ok(())
}
