use crate::dto::ProgressDto;
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
