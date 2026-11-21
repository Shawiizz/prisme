use crate::error::{AppError, AppResult};
use crate::state::AppState;
use std::sync::{Mutex, PoisonError};
use tauri::{ipc::Channel, AppHandle, State};
use tauri_plugin_updater::{Update, UpdaterExt};

/// The newer version downloaded, kept until the decks stop and the DJ installs it.
#[derive(Default)]
pub struct Downloaded(Mutex<Option<(Update, Vec<u8>)>>);

/// Downloads the newer version there is, without installing it: a deck may start meanwhile.
/// `progress` is told how much of it came, from 0 to 1.
#[tauri::command]
pub async fn download_update(
    app: AppHandle,
    downloaded: State<'_, Downloaded>,
    progress: Channel<f64>,
) -> AppResult<()> {
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
    let mut received = 0;
    let mut told = 0.0;
    let told_of = |chunk: usize, total: Option<u64>| {
        received += chunk as u64;
        let fraction = total.map_or(0.0, |total| received as f64 / total as f64);
        if fraction - told >= 0.01 {
            told = fraction;
            let _ = progress.send(fraction);
        }
    };
    let bytes = update.download(told_of, || {}).await.map_err(update_error)?;
    *downloaded.0.lock().unwrap_or_else(PoisonError::into_inner) = Some((update, bytes));
    Ok(())
}

/// Installs the version downloaded, once the decks are stopped. On Windows its installer then
/// takes over and Prisme ends at once, without the end of a normal close: Prisme is closed
/// first, as when it quits.
#[tauri::command]
pub fn install_update(state: State<'_, AppState>, downloaded: State<'_, Downloaded>) -> AppResult<()> {
    super::new_version::check_decks_stopped(&state)?;
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

/// How long the installer of a version may run on once it started it: Prisme is its last step.
#[cfg(windows)]
const INSTALLER_ENDING: std::time::Duration = std::time::Duration::from_secs(60);

/// Deletes, a moment after Prisme starts, what the updater left in the temporary folder: on
/// Windows, a folder `Prisme-<version>-updater-<random>` holding the installer of each version
/// installed, kept for it to run once Prisme was gone. On macOS and Linux it leaves nothing.
#[cfg(windows)]
pub fn clear_leftovers(app: &AppHandle) {
    let name = app.package_info().name.clone();
    let spawned = std::thread::Builder::new().name("prisme-leftovers".into()).spawn(move || {
        std::thread::sleep(INSTALLER_ENDING);
        let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else { return };
        for entry in entries.flatten() {
            let leftover = entry.file_name().to_str().is_some_and(|folder| is_leftover(folder, &name));
            if leftover && entry.file_type().is_ok_and(|t| t.is_dir()) {
                if let Err(e) = std::fs::remove_dir_all(entry.path()) {
                    tracing::warn!("{} stays: {e}", entry.path().display());
                }
            }
        }
    });
    if let Err(e) = spawned {
        tracing::warn!("the updates' leftovers stay: {e}");
    }
}

/// Whether `folder` is one the updater of application `app` made (`make_temp_dir` in
/// tauri-plugin-updater).
#[cfg(windows)]
fn is_leftover(folder: &str, app: &str) -> bool {
    let Some((version, random)) = folder
        .strip_prefix(app)
        .and_then(|rest| rest.strip_prefix('-'))
        .and_then(|rest| rest.rsplit_once("-updater-"))
    else {
        return false;
    };
    version.starts_with(|c: char| c.is_ascii_digit())
        && version.chars().all(|c| c.is_ascii_alphanumeric() || ".+-".contains(c))
        && random.len() == 6
        && random.chars().all(|c| c.is_ascii_alphanumeric())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn only_the_folders_of_its_own_updater_are_leftovers() {
        assert!(is_leftover("Prisme-0.3.9-updater-KVjD3P", "Prisme"));
        assert!(is_leftover("Prisme-0.4.0-beta.1-updater-4xtegx", "Prisme"));
        for folder in [
            "Ramure-0.1.6-updater-VkD7ZE",
            "Prismes-0.3.9-updater-KVjD3P",
            "Prisme-updater-KVjD3P",
            "Prisme--updater-KVjD3P",
            "Prisme-0.3.9-updater-",
            "Prisme-0.3.9-updater-KVjD3P7",
            "Prisme-0.3.9-updater-KVj.3P",
            "Prisme-0.3.9-installer.exe",
            "Prisme",
        ] {
            assert!(!is_leftover(folder, "Prisme"), "{folder}");
        }
    }
}
