use crate::error::{AppError, AppResult};
use crate::library_service::LibraryService;
use crate::tag_writer::TagWriter;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::State;

type Library<'a> = State<'a, Arc<LibraryService>>;

/// Colours tracks (`None` takes the colour off).
#[tauri::command]
pub fn set_track_color(library: Library<'_>, paths: Vec<String>, color: Option<u32>) -> AppResult<()> {
    let paths: Vec<&Path> = paths.iter().map(Path::new).collect();
    library.db.set_color(&paths, color)?;
    announce(&library, &paths);
    Ok(())
}

/// Rates tracks from 0 (not rated) to 5.
#[tauri::command]
pub fn set_track_rating(library: Library<'_>, paths: Vec<String>, rating: u8) -> AppResult<()> {
    let paths: Vec<&Path> = paths.iter().map(Path::new).collect();
    library.db.set_rating(&paths, rating)?;
    announce(&library, &paths);
    Ok(())
}

/// Writes `genre` into the tags of the files (empty or `None`: takes it off); the rows
/// follow as each file is written.
#[tauri::command]
pub async fn set_track_genre(
    tag_writer: State<'_, TagWriter>,
    paths: Vec<String>,
    genre: Option<String>,
) -> AppResult<()> {
    let genre = genre.map(|g| g.trim().to_owned()).filter(|g| !g.is_empty());
    let answer = tag_writer.set_genre(paths.into_iter().map(PathBuf::from).collect(), genre);
    let failed = tauri::async_runtime::spawn_blocking(move || answer.recv().unwrap_or_default())
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;
    match failed.as_slice() {
        [] => Ok(()),
        [one] => Err(AppError::Other(format!("genre not written into {one}"))),
        many => Err(AppError::Other(format!("genre not written into {} files", many.len()))),
    }
}

fn announce(library: &LibraryService, paths: &[&Path]) {
    for path in paths {
        library.announce(path);
    }
}
