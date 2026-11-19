use crate::commands::blocking;
use crate::dto::{DirEntryDto, ProgressDto};
use crate::error::AppResult;
use crate::library_service::LibraryService;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

/// State of the background analysis of the music folder.
#[tauri::command]
pub fn library_progress(library: State<'_, Arc<LibraryService>>) -> ProgressDto {
    library.progress().into()
}

/// Whether the library lives in memory only (its database could not be opened): what is
/// changed of it is lost at exit.
#[tauri::command]
pub fn library_in_memory(library: State<'_, Arc<LibraryService>>) -> bool {
    library.in_memory()
}

/// Looks for new or changed files in the music folder and analyses them. Off the main thread:
/// the analysis running stops first.
#[tauri::command]
pub async fn rescan_library(app: AppHandle) -> AppResult<()> {
    blocking(move || {
        app.state::<Arc<LibraryService>>().start();
        Ok(())
    })
    .await
}

/// Most results a search returns: more would not be read, and would slow the list.
const SEARCH_LIMIT: usize = 500;

/// Tracks of the whole library matching `query` (tags and file names); every track of it when
/// nothing is searched.
#[tauri::command(async)]
pub fn search_library(library: State<'_, Arc<LibraryService>>, query: String) -> AppResult<Vec<DirEntryDto>> {
    let records = if query.trim().is_empty() {
        library.db.analysed_records()?
    } else {
        library.db.search(&query, SEARCH_LIMIT)?
    };
    Ok(records.iter().map(|r| DirEntryDto::track(&r.path, Some(r))).collect())
}

/// The tracks of the library under none of the music folders: loaded from elsewhere, they stay
/// until removed. Those of a music folder that is away (a drive unplugged) are not among them.
#[tauri::command(async)]
pub fn elsewhere_tracks(library: State<'_, Arc<LibraryService>>) -> AppResult<Vec<DirEntryDto>> {
    let records = library.db.records_outside(&library.roots())?;
    Ok(records.iter().map(|r| DirEntryDto::track(&r.path, Some(r))).collect())
}

/// Takes `paths` out of the library, their files left as they are: their cues, corrections and
/// crate entries are kept for when the same audio comes back. One in a music folder comes back
/// at the next scan.
#[tauri::command(async)]
pub fn remove_tracks(library: State<'_, Arc<LibraryService>>, paths: Vec<String>) -> AppResult<()> {
    Ok(library.db.forget(&paths)?)
}

/// The genres of the library, the most common first (suggestions when tagging).
#[tauri::command(async)]
pub fn library_genres(library: State<'_, Arc<LibraryService>>) -> AppResult<Vec<String>> {
    Ok(library.db.genres()?)
}

/// Tracks that look like the same song, one group after the other.
#[tauri::command(async)]
pub fn find_duplicates(library: State<'_, Arc<LibraryService>>) -> AppResult<Vec<DirEntryDto>> {
    let groups = prisme_library::duplicate_groups(library.db.analysed_records()?);
    Ok(groups.iter().flatten().map(|r| DirEntryDto::track(&r.path, Some(r))).collect())
}
