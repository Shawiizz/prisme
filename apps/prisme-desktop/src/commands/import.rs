//! Import from Serato and rekordbox (see `prisme_import`).

use crate::commands::blocking;
use crate::dto::ImportSummaryDto;
use crate::error::AppResult;
use crate::library_service::LibraryService;
use prisme_import::{rekordbox, serato, Import};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::State;

/// The `_Serato_` folder of the music folder, else of the system's, if there is one.
#[tauri::command]
pub fn serato_folder(library: State<'_, Arc<LibraryService>>) -> Option<String> {
    [library.root(), crate::folders::system_music_root()]
        .into_iter()
        .map(|dir| dir.join("_Serato_"))
        .find(|dir| dir.is_dir())
        .map(|dir| dir.display().to_string())
}

/// Crates of `serato_dir` (if given), and the Serato cues, loops, colours and grids of
/// the files of the music folder and of those crates.
#[tauri::command]
pub async fn import_serato(
    library: State<'_, Arc<LibraryService>>,
    serato_dir: Option<String>,
) -> AppResult<ImportSummaryDto> {
    let library = library.inner().clone();
    let root = library.root();
    run(library, move || {
        let files: Vec<PathBuf> = prisme_library::scan_folder(&root, &AtomicBool::new(false))
            .files
            .into_iter()
            .map(|f| f.path)
            .collect();
        Ok(serato::read(serato_dir.as_deref().map(std::path::Path::new), &files)?)
    })
    .await
}

/// The collection and playlists of a rekordbox XML export.
#[tauri::command]
pub async fn import_rekordbox(
    library: State<'_, Arc<LibraryService>>,
    path: String,
) -> AppResult<ImportSummaryDto> {
    let library = library.inner().clone();
    run(library, move || Ok(rekordbox::read(std::path::Path::new(&path))?)).await
}

/// Reads off the main thread (tags of the whole library), then merges.
async fn run(
    library: Arc<LibraryService>,
    read: impl FnOnce() -> AppResult<Import> + Send + 'static,
) -> AppResult<ImportSummaryDto> {
    blocking(move || {
        let import = read()?;
        let summary = prisme_import::apply(&library.db, &import)?;
        for track in &import.tracks {
            library.announce(&track.path);
        }
        Ok(summary.into())
    })
    .await
}
