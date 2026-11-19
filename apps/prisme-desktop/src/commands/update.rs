use crate::error::{AppError, AppResult};
use std::sync::{Mutex, PoisonError};
use tauri::{AppHandle, State};
use tauri_plugin_updater::{Update, UpdaterExt};

/// The newer version downloaded, kept until the decks stop and the DJ installs it.
#[derive(Default)]
pub struct Downloaded(Mutex<Option<(Update, Vec<u8>)>>);

/// Downloads the newer version there is, without installing it: a deck may start meanwhile.
#[tauri::command]
pub async fn download_update(app: AppHandle, downloaded: State<'_, Downloaded>) -> AppResult<()> {
    let closing = app.clone();
    let updater = app
        .updater_builder()
        .on_before_exit(move || {
            crate::close(&closing);
            closing.cleanup_before_exit();
        })
        .build()
        .map_err(update_error)?;
    let update = updater
        .check()
        .await
        .map_err(update_error)?
        .ok_or_else(|| AppError::Other("no newer version".into()))?;
    let bytes = update.download(|_, _| {}, || {}).await.map_err(update_error)?;
    *downloaded.0.lock().unwrap_or_else(PoisonError::into_inner) = Some((update, bytes));
    Ok(())
}

/// Installs the version downloaded. On Windows its installer then takes over and Prisme ends
/// at once, without the end of a normal close: Prisme is closed first, as when it quits.
#[tauri::command]
pub fn install_update(downloaded: State<'_, Downloaded>) -> AppResult<()> {
    let (update, bytes) = downloaded
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .ok_or_else(|| AppError::Other("no newer version downloaded".into()))?;
    update.install(bytes).map_err(update_error)
}

fn update_error(e: tauri_plugin_updater::Error) -> AppError {
    AppError::Other(e.to_string())
}
