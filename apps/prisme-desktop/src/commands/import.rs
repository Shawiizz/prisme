//! Import from Serato and rekordbox (see `prisme_import`).

use crate::commands::blocking;
use crate::dto::ImportSummaryDto;
use crate::error::AppResult;
use crate::library_service::LibraryService;
use crate::picked;
use prisme_import::{rekordbox, serato, Import};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::State;
use tauri_plugin_dialog::FilePath;

/// The `_Serato_` folder of the music folder, else of the system's, if there is one.
#[tauri::command]
pub fn serato_folder(library: State<'_, Arc<LibraryService>>) -> Option<String> {
    [library.root(), crate::folders::system_music_root()]
        .into_iter()
        .map(|dir| dir.join("_Serato_"))
        .find(|dir| dir.is_dir())
        .map(|dir| dir.display().to_string())
}

/// Crates of `serato_dir` (if given), and the Serato cues, loops, colours and (with `grids`)
/// grids of the files of the music folders and of those crates.
#[tauri::command]
pub async fn import_serato(
    library: State<'_, Arc<LibraryService>>,
    serato_dir: Option<String>,
    grids: bool,
) -> AppResult<ImportSummaryDto> {
    let library = library.inner().clone();
    let roots = library.roots();
    run(library, grids, move || {
        let files: Vec<PathBuf> = prisme_library::scan_folders(&roots, &AtomicBool::new(false))
            .files
            .into_iter()
            .map(|f| f.path)
            .collect();
        Ok(serato::read(serato_dir.as_deref().map(std::path::Path::new), &files)?)
    })
    .await
}

/// The collection and playlists of a rekordbox XML export, its beat grids with `grids`.
#[tauri::command]
pub async fn import_rekordbox(
    library: State<'_, Arc<LibraryService>>,
    path: FilePath,
    grids: bool,
) -> AppResult<ImportSummaryDto> {
    let library = library.inner().clone();
    run(library, grids, move || Ok(rekordbox::read(picked::open(path)?)?)).await
}

/// Reads off the main thread (tags of the whole library), then merges: the beat grids too with
/// `grids`, else the tracks keep those of Prisme's analysis.
async fn run(
    library: Arc<LibraryService>,
    grids: bool,
    read: impl FnOnce() -> AppResult<Import> + Send + 'static,
) -> AppResult<ImportSummaryDto> {
    blocking(move || {
        let import = read()?;
        let import = if grids { import } else { import.without_grids() };
        let summary = prisme_import::apply(&library.db, &import)?;
        for track in &import.tracks {
            library.announce(&track.path);
        }
        library.lists_changed();
        Ok(summary.into())
    })
    .await
}
