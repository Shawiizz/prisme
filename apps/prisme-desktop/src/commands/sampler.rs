//! A deck's sampler (see `sampler`).

use super::blocking;
use crate::actions::sampler;
use crate::error::{AppError, AppResult};
use crate::picked;
use crate::state::AppState;
use tauri::{AppHandle, Manager};
use tauri_plugin_fs::FilePath;

/// The sound of `slot` stopped if it plays, else taken off its pad (a right click on it).
#[tauri::command]
pub fn take_sample_off(app: AppHandle, deck: usize, slot: usize) -> AppResult<()> {
    sampler::take_off(&app.state::<AppState>(), deck, slot)
}

/// Puts the sound of a file the DJ picked in a slot; kept for the next start. One with no path
/// (Android's) is copied among Prisme's files first, to be found again then.
#[tauri::command]
pub async fn load_sample(app: AppHandle, deck: usize, slot: usize, path: FilePath) -> AppResult<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        let uri = match path {
            FilePath::Path(path) => return sampler::load_sample(&state, deck, slot, path),
            FilePath::Url(uri) => uri,
        };
        sampler::check_slot(deck, slot)?;
        let samples =
            crate::data_dir(&app).ok_or_else(|| AppError::Other("no data folder".into()))?.join("samples");
        let name = picked::file_name(&uri);
        let sound = picked::open(&app, FilePath::Url(uri))?;
        let (path, audio) = crate::sampler::keep_copy(&samples, deck, slot, &name, sound)?;
        sampler::keep_sample(&state, deck, slot, path, audio)
    })
    .await
}
