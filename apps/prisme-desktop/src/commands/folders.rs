use crate::dto::{DirListingDto, TrackDetailsDto};
use crate::error::AppResult;
use crate::folders;
use crate::library_service::LibraryService;
use prisme_library::FileStamp;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn music_root() -> String {
    folders::music_root().display().to_string()
}

/// Folders and tracks of a folder, with the stored analysis of each track.
#[tauri::command]
pub fn list_dir(library: State<'_, Arc<LibraryService>>, path: String) -> AppResult<DirListingDto> {
    Ok(folders::list_dir(&PathBuf::from(path), |file, meta| {
        let record = library.db.get(file).ok().flatten()?;
        record.is_current(FileStamp::from_metadata(meta)).then(|| TrackDetailsDto::from(&record))
    })?)
}
