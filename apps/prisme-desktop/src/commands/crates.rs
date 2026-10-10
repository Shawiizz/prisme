use crate::dto::{CrateDto, DirEntryDto};
use crate::error::{AppError, AppResult};
use crate::library_service::LibraryService;
use prisme_library::CrateId;
use std::path::Path;
use std::sync::Arc;
use tauri::State;

type Library<'a> = State<'a, Arc<LibraryService>>;

#[tauri::command]
pub fn list_crates(library: Library<'_>) -> AppResult<Vec<CrateDto>> {
    Ok(library.db.crates()?.into_iter().map(CrateDto::from).collect())
}

#[tauri::command]
pub fn create_crate(library: Library<'_>, name: String) -> AppResult<i64> {
    Ok(library.db.create_crate(crate_name(&name)?)?.0)
}

#[tauri::command]
pub fn rename_crate(library: Library<'_>, id: i64, name: String) -> AppResult<()> {
    Ok(library.db.rename_crate(CrateId(id), crate_name(&name)?)?)
}

#[tauri::command]
pub fn delete_crate(library: Library<'_>, id: i64) -> AppResult<()> {
    Ok(library.db.delete_crate(CrateId(id))?)
}

/// Returns how many tracks were not in the crate yet.
#[tauri::command]
pub fn add_to_crate(library: Library<'_>, id: i64, paths: Vec<String>) -> AppResult<usize> {
    Ok(library.db.add_to_crate(CrateId(id), &as_paths(&paths))?)
}

#[tauri::command]
pub fn remove_from_crate(library: Library<'_>, id: i64, paths: Vec<String>) -> AppResult<()> {
    Ok(library.db.remove_from_crate(CrateId(id), &as_paths(&paths))?)
}

#[tauri::command]
pub fn crate_tracks(library: Library<'_>, id: i64) -> AppResult<Vec<DirEntryDto>> {
    let entries = library.db.crate_tracks(CrateId(id))?;
    Ok(entries.iter().map(|e| DirEntryDto::track(&e.path, e.record.as_ref())).collect())
}

fn crate_name(name: &str) -> AppResult<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::EmptyCrateName);
    }
    Ok(name)
}

fn as_paths(paths: &[String]) -> Vec<&Path> {
    paths.iter().map(Path::new).collect()
}
