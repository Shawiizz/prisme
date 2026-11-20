use crate::commands::blocking;
use crate::dto::{DirListingDto, FolderDto, MusicFolderDto, StoragePlaceDto};
use crate::error::AppResult;
use crate::folders;
use crate::library_service::LibraryService;
use crate::state::AppState;
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
        app.state::<AppState>().update_settings(|p| p.library.music_folder = chosen.clone());
        let library = app.state::<Arc<LibraryService>>();
        library.move_to(folders::music_root(chosen.as_deref()));
        Ok(library.root().display().to_string())
    })
    .await
}

/// The music folders: the main one first, then those added, in the order they were. Off the
/// main thread: whether each is there is asked of its drive (a NAS may be slow to answer).
#[tauri::command(async)]
pub fn music_folders(library: State<'_, Arc<LibraryService>>) -> Vec<MusicFolderDto> {
    library.folders()
}

/// Adds `path` to the music folders, scanned and watched at once; returns the music folders.
/// Off the main thread: the analysis running stops first.
#[tauri::command]
pub async fn add_music_folder(app: AppHandle, path: String) -> AppResult<Vec<MusicFolderDto>> {
    blocking(move || {
        let library = app.state::<Arc<LibraryService>>();
        library.add(Path::new(&path))?;
        Ok(library.folders())
    })
    .await
}

/// Removes `path`, added before, from the music folders; its files stay as they are, its
/// tracks leave the library. Returns the music folders. Off the main thread: the analysis
/// running stops first.
#[tauri::command]
pub async fn remove_music_folder(app: AppHandle, path: String) -> AppResult<Vec<MusicFolderDto>> {
    blocking(move || {
        let library = app.state::<Arc<LibraryService>>();
        library.remove(Path::new(&path))?;
        Ok(library.folders())
    })
    .await
}

/// Shows `path` in the system's file manager: a folder opened, a file selected in its own.
#[tauri::command]
pub fn show_in_file_manager(path: String) -> AppResult<()> {
    Ok(folders::show(Path::new(&path))?)
}

/// Where a folder to add is looked for first: the phone's storage and its memory cards and USB
/// drives, or the home folder and the drives of a computer. Off the main thread: a drive may
/// be slow to answer.
#[tauri::command(async)]
pub fn storage_places() -> Vec<StoragePlaceDto> {
    folders::storage_places()
}

/// The folders in a folder, and how many tracks: the sub-folders of the side panel's trees, a
/// folder looked into to add it.
#[tauri::command(async)]
pub fn list_dir(library: State<'_, Arc<LibraryService>>, path: String) -> AppResult<DirListingDto> {
    let path = PathBuf::from(path);
    let listing = folders::listing(&path, &library.roots())?;
    Ok(DirListingDto {
        path: path.display().to_string(),
        parent: path.parent().map(|p| p.display().to_string()),
        folders: listing.folders.iter().map(|folder| FolderDto::of(folder)).collect(),
        tracks: listing.files.len(),
    })
}
