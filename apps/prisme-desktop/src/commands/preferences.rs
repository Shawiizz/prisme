use crate::preferences::{DjPreferences, LibraryPreferences, Preferences};
use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub fn get_preferences(state: State<'_, AppState>) -> Preferences {
    state.preferences.get()
}

#[tauri::command]
pub fn set_dj_preferences(state: State<'_, AppState>, dj: DjPreferences) {
    state.update_preferences(|p| p.dj = dj);
}

#[tauri::command]
pub fn set_library_preferences(state: State<'_, AppState>, library: LibraryPreferences) {
    state.update_preferences(|p| p.library = library);
}
