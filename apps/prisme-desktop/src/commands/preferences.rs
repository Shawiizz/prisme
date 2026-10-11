use crate::error::AppResult;
use crate::preferences::{DjPreferences, LibraryPreferences, Preferences};
use crate::state::AppState;
use crate::tag_writer::TagWriter;
use prisme_engine::Command;
use tauri::State;

#[tauri::command]
pub fn get_preferences(state: State<'_, AppState>) -> Preferences {
    state.preferences.get()
}

#[tauri::command]
pub fn set_dj_preferences(state: State<'_, AppState>, dj: DjPreferences) -> AppResult<()> {
    state.update_preferences(|p| p.dj = dj);
    state.send_if_running(Command::SetQuantize { enabled: dj.quantize })
}

#[tauri::command]
pub fn set_library_preferences(
    state: State<'_, AppState>,
    tag_writer: State<'_, TagWriter>,
    library: LibraryPreferences,
) {
    let write_tags = library.write_tags;
    state.update_preferences(|p| p.library = library);
    if write_tags {
        tag_writer.wake();
    }
}
