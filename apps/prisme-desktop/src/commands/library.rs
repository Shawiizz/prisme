use crate::dto::{DirEntryDto, ProgressDto};
use crate::error::AppResult;
use crate::library_service::LibraryService;
use std::sync::Arc;
use tauri::State;

/// State of the background analysis of the music folder.
#[tauri::command]
pub fn library_progress(library: State<'_, Arc<LibraryService>>) -> ProgressDto {
    library.progress().into()
}

/// Looks for new or changed files in the music folder and analyses them.
#[tauri::command]
pub fn rescan_library(library: State<'_, Arc<LibraryService>>) {
    library.start();
}

/// Most results a search returns: more would not be read, and would slow the list.
const SEARCH_LIMIT: usize = 500;

/// Tracks of the whole library matching `query` (tags and file names).
#[tauri::command]
pub fn search_library(library: State<'_, Arc<LibraryService>>, query: String) -> AppResult<Vec<DirEntryDto>> {
    Ok(library
        .db
        .search(&query, SEARCH_LIMIT)?
        .iter()
        .map(|r| DirEntryDto::track(&r.path, Some(r)))
        .collect())
}
