use crate::dto::DirEntryDto;
use crate::error::AppResult;
use crate::library_service::LibraryService;
use crate::state::AppState;
use crate::stems_service::StemsService;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::State;

type Library<'a> = State<'a, Arc<LibraryService>>;

#[tauri::command]
pub fn prepare_list(library: Library<'_>) -> AppResult<Vec<DirEntryDto>> {
    let entries = library.db.prepare_list()?;
    Ok(entries.iter().map(|e| DirEntryDto::track(&e.path, e.record.as_ref())).collect())
}

/// Returns how many tracks were not in the list yet. The tracks are separated in the
/// background if the preference says so: ready before the set.
#[tauri::command(async)]
pub fn add_to_prepare(
    library: Library<'_>,
    state: State<'_, AppState>,
    stems: State<'_, StemsService>,
    paths: Vec<String>,
) -> AppResult<usize> {
    let added = library.db.add_to_prepare(&paths.iter().map(Path::new).collect::<Vec<_>>())?;
    if state.preferences.read(|p| p.library.separate_on_load) {
        // without a model there is nothing to do, and nothing to tell
        let _ = stems.enqueue(paths.into_iter().map(PathBuf::from));
    }
    Ok(added)
}

#[tauri::command]
pub fn remove_from_prepare(library: Library<'_>, paths: Vec<String>) -> AppResult<()> {
    library.db.remove_from_prepare(&paths.iter().map(Path::new).collect::<Vec<_>>())?;
    Ok(())
}

#[tauri::command]
pub fn move_in_prepare(library: Library<'_>, path: String, index: usize) -> AppResult<()> {
    Ok(library.db.move_in_prepare(Path::new(&path), index)?)
}
