use crate::analysis_service::AnalysisService;
use crate::dto::{DirListingDto, ProgressDto, TrackAnalysisDto};
use crate::error::AppResult;
use crate::library;
use prisme_library::FileStamp;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn music_root() -> String {
    library::music_root().display().to_string()
}

/// Folders and tracks of a folder, with the stored analysis of each track.
#[tauri::command]
pub fn list_dir(library: State<'_, Arc<AnalysisService>>, path: String) -> AppResult<DirListingDto> {
    Ok(library::list_dir(&PathBuf::from(path), |file, meta| {
        let record = library.db.get(file).ok().flatten()?;
        record.is_current(FileStamp::from_metadata(meta)).then(|| TrackAnalysisDto::from(&record))
    })?)
}

/// State of the background analysis of the music folder.
#[tauri::command]
pub fn library_progress(library: State<'_, Arc<AnalysisService>>) -> ProgressDto {
    library.progress().into()
}

/// Looks for new or changed files in the music folder and analyses them.
#[tauri::command]
pub fn rescan_library(library: State<'_, Arc<AnalysisService>>) {
    library.start();
}
