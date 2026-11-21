use crate::commands::blocking;
use crate::dto::{DirListingDto, FolderDto, StoragePlaceDto};
use crate::error::AppResult;
use crate::folders;
use crate::library_service::LibraryService;
use crate::state::AppState;
use prisme_library::root_of;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

/// Files and folders dropped from the computer's file manager: the tracks Prisme plays, the
/// rest taken for folders.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "Dropped"))]
pub struct DroppedDto {
    pub tracks: Vec<String>,
    pub folders: Vec<String>,
}

/// `paths` dropped, sorted into tracks and folders by their names (the files may not be read
/// yet: a drag only shows them).
#[tauri::command]
pub fn sort_dropped(paths: Vec<String>) -> DroppedDto {
    let (tracks, folders) = paths.into_iter().partition(|p| prisme_media::is_supported(Path::new(p)));
    DroppedDto { tracks, folders }
}

/// The extensions of the files Prisme plays, for a file dialog.
#[tauri::command]
pub fn audio_extensions() -> &'static [&'static str] {
    prisme_media::SUPPORTED_EXTENSIONS
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

/// Adds `path` to the music folders, scanned and watched at once. Off the main thread: the
/// analysis running stops first.
#[tauri::command]
pub async fn add_music_folder(app: AppHandle, path: String) -> AppResult<()> {
    blocking(move || app.state::<Arc<LibraryService>>().add(Path::new(&path))).await
}

/// Removes `path`, added before, from the music folders; its files stay as they are, its
/// tracks leave the library. Off the main thread: the analysis running stops first.
#[tauri::command]
pub async fn remove_music_folder(app: AppHandle, path: String) -> AppResult<()> {
    blocking(move || app.state::<Arc<LibraryService>>().remove(Path::new(&path))).await
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
    let roots = library.roots();
    let listing = folders::listing(&path, &roots)?;
    // Android's data folder, at the top of a place, holds the applications' files, not music
    let top =
        cfg!(target_os = "android") && folders::storage_places().iter().any(|p| Path::new(&p.path) == path);
    let shown = |folder: &&PathBuf| !(top && folder.file_name().is_some_and(|name| name == "Android"));
    Ok(DirListingDto {
        within: root_of(&path, &roots).map(|root| root.display().to_string()),
        path: path.display().to_string(),
        parent: path.parent().map(|p| p.display().to_string()),
        folders: listing.folders.iter().filter(shown).map(|folder| FolderDto::of(folder)).collect(),
        tracks: listing.files.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_files_dropped_are_tracks_by_their_extension_the_rest_folders() {
        let dropped = sort_dropped(
            ["C:/Music/Track.FLAC", "C:/Music/House.mp3s", "C:/Music/cover.jpg", "/dj/a.mp3"]
                .map(String::from)
                .to_vec(),
        );
        assert_eq!(dropped.tracks, ["C:/Music/Track.FLAC", "/dj/a.mp3"]);
        assert_eq!(dropped.folders, ["C:/Music/House.mp3s", "C:/Music/cover.jpg"]);
    }
}
