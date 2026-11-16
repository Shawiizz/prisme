use crate::commands::blocking;
use crate::dto::{DirListingDto, TrackDetailsDto};
use crate::error::AppResult;
use crate::folders;
use crate::library_service::LibraryService;
use crate::state::AppState;
use prisme_library::{FileStamp, Search};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

#[tauri::command]
pub fn music_root(library: State<'_, Arc<LibraryService>>) -> String {
    library.root().display().to_string()
}

/// Makes `path` the music folder (`None`: the system's), scanned at once; returns the music
/// folder. Off the main thread: the analysis running stops first.
#[tauri::command]
pub async fn set_music_folder(app: AppHandle, path: Option<String>) -> AppResult<String> {
    blocking(move || {
        let chosen = path.filter(|path| Path::new(path) != folders::system_music_root());
        app.state::<AppState>().update_preferences(|p| p.library.music_folder = chosen.clone());
        let library = app.state::<Arc<LibraryService>>();
        library.move_to(folders::music_root(chosen.as_deref()));
        Ok(library.root().display().to_string())
    })
    .await
}

/// Folders and tracks of a folder, with the stored analysis of each track.
#[tauri::command(async)]
pub fn list_dir(library: State<'_, Arc<LibraryService>>, path: String) -> AppResult<DirListingDto> {
    Ok(folders::list_dir(&PathBuf::from(path), &library.root(), |file, meta| {
        let stamp = FileStamp::from_metadata(meta);
        // a folder that moved, or came back under another drive letter, keeps what was known
        let _ = prisme_library::adopt(&library.db, file, stamp, Search::Stamp);
        let record = library.db.get(file).ok().flatten()?;
        record.is_current(stamp).then(|| TrackDetailsDto::from(&record))
    })?)
}
