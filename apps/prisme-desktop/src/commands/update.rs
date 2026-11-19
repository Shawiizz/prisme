use crate::error::{AppError, AppResult};
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

/// Downloads and installs the newer version there is. On Windows its installer then takes
/// over and Prisme ends at once, without the end of a normal close: Prisme is closed first,
/// as when it quits.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> AppResult<()> {
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
    update.download_and_install(|_, _| {}, || {}).await.map_err(update_error)
}

fn update_error(e: tauri_plugin_updater::Error) -> AppError {
    AppError::Other(e.to_string())
}
