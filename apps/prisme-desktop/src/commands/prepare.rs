use crate::dto::DirEntryDto;
use crate::error::AppResult;
use crate::library_service::LibraryService;
use std::path::Path;
use std::sync::Arc;
use tauri::State;

type Library<'a> = State<'a, Arc<LibraryService>>;

#[tauri::command]
pub fn prepare_list(library: Library<'_>) -> AppResult<Vec<DirEntryDto>> {
    let entries = library.db.prepare_list()?;
    Ok(entries.iter().map(|e| DirEntryDto::track(&e.path, e.record.as_ref())).collect())
}

/// Returns how many tracks were not in the list yet.
#[tauri::command]
pub fn add_to_prepare(library: Library<'_>, paths: Vec<String>) -> AppResult<usize> {
    Ok(library.db.add_to_prepare(&paths.iter().map(Path::new).collect::<Vec<_>>())?)
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
