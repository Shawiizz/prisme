use crate::error::AppResult;
use crate::library_service::LibraryService;
use std::path::Path;
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

fn announce(library: &LibraryService, paths: &[&Path]) {
    for path in paths {
        library.announce(path);
    }
}
