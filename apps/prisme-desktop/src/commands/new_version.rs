//! Newer versions of Prisme: looked for on the releases, installed only once no deck plays (on a
//! computer `update`, on Android `apk`).

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use serde::Serialize;
use tauri::AppHandle;

/// The latest release, from GitHub's API.
#[cfg(target_os = "android")]
const LATEST: &str = "https://api.github.com/repos/Shawiizz/prisme/releases/latest";

/// A newer version than the one running: installed in place on a computer, its APK handed to
/// Android on a phone.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "NewVersion"))]
pub struct NewVersionDto {
    pub version: String,
    pub apk: Option<ApkDto>,
}

/// The APK of a release, and its size in bytes.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "Apk"))]
pub struct ApkDto {
    pub url: String,
    pub size: u64,
}

/// The newer version published, if there is one: by the updater on a computer, from the latest
/// release's APK on Android; none on iOS, updated by its store.
#[tauri::command]
pub async fn check_update(app: AppHandle) -> AppResult<Option<NewVersionDto>> {
    #[cfg(desktop)]
    {
        use tauri_plugin_updater::UpdaterExt;
        let failed = |e: tauri_plugin_updater::Error| AppError::Other(e.to_string());
        let update = app.updater().map_err(failed)?.check().await.map_err(failed)?;
        Ok(update.map(|u| NewVersionDto { version: u.version, apk: None }))
    }
    #[cfg(target_os = "android")]
    {
        let client = crate::http::client(std::time::Duration::from_secs(30)).map_err(AppError::Other)?;
        let release = crate::http::fetch(&client, LATEST).await.map_err(AppError::Other)?;
        let release: serde_json::Value =
            serde_json::from_slice(&release).map_err(|e| AppError::Other(e.to_string()))?;
        Ok(newer_apk(&release, &app.package_info().version.to_string()))
    }
    #[cfg(target_os = "ios")]
    {
        let _ = app;
        Ok(None)
    }
}

/// Refused while a deck plays: a new version installs once the DJ stopped them all.
pub fn decks_stopped(state: &AppState) -> AppResult<()> {
    if state.is_playing() {
        Err(AppError::DecksPlay)
    } else {
        Ok(())
    }
}

/// The APK of the `release` GitHub describes, if it is a newer version than `running`.
#[cfg(any(target_os = "android", test))]
fn newer_apk(release: &serde_json::Value, running: &str) -> Option<NewVersionDto> {
    let tag = release["tag_name"].as_str()?;
    let assets = release["assets"].as_array()?;
    let apk = assets.iter().find(|a| a["name"].as_str().is_some_and(|n| n.ends_with(".apk")))?;
    let apk = ApkDto { url: apk["browser_download_url"].as_str()?.to_owned(), size: apk["size"].as_u64()? };
    newer(tag, running).then(|| NewVersionDto { version: tag.to_owned(), apk: Some(apk) })
}

/// Whether version `a` (`1.2.3`) comes after `b`, number by number.
#[cfg(any(target_os = "android", test))]
fn newer(a: &str, b: &str) -> bool {
    let numbers = |v: &str| -> Vec<u64> { v.split('.').map(|n| n.parse().unwrap_or(0)).collect() };
    let (a, b) = (numbers(a), numbers(b));
    let at = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
    let order = (0..a.len().max(b.len())).map(|i| at(&a, i).cmp(&at(&b, i))).find(|o| o.is_ne());
    order == Some(std::cmp::Ordering::Greater)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_is_newer_number_by_number_not_by_its_text() {
        assert!(newer("0.3.10", "0.3.9"));
        assert!(newer("0.4.0", "0.3.12"));
        assert!(newer("1.0.0", "0.9.9"));
        assert!(!newer("0.3.6", "0.3.6"));
        assert!(!newer("0.3.5", "0.3.6"));
    }

    #[test]
    fn the_latest_release_offers_its_apk_when_newer() {
        let release = serde_json::json!({
            "tag_name": "0.4.0",
            "assets": [
                { "name": "Prisme_0.4.0_x64-setup.exe", "browser_download_url": "x.exe", "size": 2 },
                { "name": "Prisme_0.4.0_android-arm64.apk", "browser_download_url": "x.apk", "size": 3 }
            ]
        });
        let apk = ApkDto { url: "x.apk".into(), size: 3 };
        let found = NewVersionDto { version: "0.4.0".into(), apk: Some(apk) };
        assert_eq!(newer_apk(&release, "0.3.9"), Some(found));
        assert_eq!(newer_apk(&release, "0.4.0"), None);
    }

    #[test]
    fn a_new_version_waits_for_the_decks_to_stop() {
        let bench = crate::actions::bench::Bench::new();
        assert!(decks_stopped(&bench.state).is_ok());
        bench.load(0, 30.0, Some(120.0));
        bench.press("deck1.play", 1.0).unwrap();
        bench.run(0.1);
        assert!(matches!(decks_stopped(&bench.state), Err(AppError::DecksPlay)));
    }
}
