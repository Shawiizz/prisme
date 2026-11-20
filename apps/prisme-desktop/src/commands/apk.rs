//! A new version on Android, where Prisme cannot install itself: the APK of the release is
//! downloaded into the cache, then handed to Android's installer (`Update.kt`), where the DJ
//! confirms. Installed, it ends Prisme, whose next start empties the folder.

#[cfg(target_os = "android")]
use crate::android_jni;
#[cfg(target_os = "android")]
use crate::error::{AppError, AppResult};
#[cfg(target_os = "android")]
use jni::objects::{JClass, JObject, JValue};
#[cfg(target_os = "android")]
use jni::JNIEnv;
#[cfg(target_os = "android")]
use std::io::Write;
#[cfg(target_os = "android")]
use std::path::PathBuf;
#[cfg(target_os = "android")]
use tauri::{ipc::Channel, AppHandle, Manager};

/// Where the files of the releases are: nothing else is downloaded.
const RELEASES: &str = "https://github.com/Shawiizz/prisme/releases/download/";

/// The name of the APK `url` downloads, if it is one of a release.
fn release_apk(url: &str) -> Option<&str> {
    let (tag, name) = url.strip_prefix(RELEASES)?.split_once('/')?;
    let plain = |part: &str| {
        !part.is_empty()
            && !part.starts_with('.')
            && part.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    };
    (plain(tag) && plain(name) && name.ends_with(".apk")).then_some(name)
}

#[cfg(target_os = "android")]
const CLASS: &str = "com.shawiizz.prisme.Update";
/// `Update.install`'s answers.
#[cfg(target_os = "android")]
const OPENED: i32 = 0;
#[cfg(target_os = "android")]
const NOT_ALLOWED: i32 = 1;
#[cfg(target_os = "android")]
const NOT_PRISME: i32 = 2;
/// A stalled download gives up after this long.
#[cfg(target_os = "android")]
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// The folder of the cache the APK goes into.
#[cfg(target_os = "android")]
fn folder(app: &AppHandle) -> AppResult<PathBuf> {
    let cache = app.path().app_cache_dir().map_err(|e| AppError::Other(e.to_string()))?;
    Ok(cache.join("updates"))
}

/// Empties the folder of the APKs as Prisme starts: the version that runs is the one
/// installed from it, or the DJ did not install it.
#[cfg(target_os = "android")]
pub fn clear_leftovers(app: &AppHandle) {
    let Ok(folder) = folder(app) else { return };
    match std::fs::remove_dir_all(&folder) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            tracing::warn!("{} stays: {e}", folder.display())
        }
        _ => {}
    }
}

/// Where the APK `url` downloads goes.
#[cfg(target_os = "android")]
fn apk_path(app: &AppHandle, url: &str) -> AppResult<PathBuf> {
    let name = release_apk(url).ok_or_else(|| AppError::Other(format!("not a release of Prisme: {url}")))?;
    Ok(folder(app)?.join(name))
}

/// Downloads the APK of a release, `size` bytes, telling `progress` how much of it came (from
/// 0 to 1). Kept until Prisme starts again: a deck may start meanwhile, the DJ installing it
/// once stopped, and an APK already there is not downloaded again.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn download_apk(app: AppHandle, url: String, size: u64, progress: Channel<f64>) -> AppResult<()> {
    let target = apk_path(&app, &url)?;
    if std::fs::metadata(&target).is_ok_and(|m| m.len() == size) {
        let _ = progress.send(1.0);
        return Ok(());
    }
    let _ = std::fs::remove_dir_all(folder(&app)?);
    std::fs::create_dir_all(folder(&app)?)?;
    let partial = target.with_extension("part");
    let fetched = fetch(&url, &partial, size, &progress).await;
    if let Err(e) = fetched {
        let _ = std::fs::remove_file(&partial);
        return Err(e);
    }
    std::fs::rename(&partial, &target)?;
    Ok(())
}

/// Writes the file at `url` into `partial`, refused unless it is `size` bytes long.
#[cfg(target_os = "android")]
async fn fetch(url: &str, partial: &std::path::Path, size: u64, progress: &Channel<f64>) -> AppResult<()> {
    let client = crate::http::client(TIMEOUT).map_err(AppError::Other)?;
    let response = client.get(url).send().await.and_then(|r| r.error_for_status());
    let mut response = response.map_err(|e| AppError::Other(e.to_string()))?;
    let mut file = std::fs::File::create(partial)?;
    let mut received = 0;
    let mut told = 0.0;
    while let Some(chunk) = response.chunk().await.map_err(|e| AppError::Other(e.to_string()))? {
        received += chunk.len() as u64;
        if received > size {
            return Err(AppError::NotTheUpdate);
        }
        file.write_all(&chunk)?;
        let fraction = received as f64 / size as f64;
        if fraction - told >= 0.01 {
            told = fraction;
            let _ = progress.send(fraction);
        }
    }
    if received != size {
        return Err(AppError::NotTheUpdate);
    }
    Ok(file.sync_all()?)
}

/// What became of the APK handed to Android.
#[cfg(target_os = "android")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Handed {
    /// Its installer shows it, for the DJ to confirm.
    Installer,
    /// The DJ has yet to let Prisme install applications ([`allow_installs`]).
    NotAllowed,
}

/// Hands the APK `url` downloaded to Android's installer. One that is no version of Prisme
/// goes.
#[cfg(target_os = "android")]
#[tauri::command(async)]
pub fn install_apk(app: AppHandle, url: String) -> AppResult<Handed> {
    super::new_version::decks_stopped(&app.state::<crate::state::AppState>())?;
    let path = apk_path(&app, &url)?;
    let answer = call("hand the new version to Android", |env, class| {
        let path = JObject::from(env.new_string(path.to_string_lossy())?);
        env.call_static_method(class, "install", "(Ljava/lang/String;)I", &[JValue::Object(&path)])?.i()
    });
    match answer {
        Some(OPENED) => Ok(Handed::Installer),
        Some(NOT_ALLOWED) => Ok(Handed::NotAllowed),
        Some(NOT_PRISME) => {
            let _ = std::fs::remove_file(&path);
            Err(AppError::NotTheUpdate)
        }
        _ => Err(AppError::Other("Android's installer could not be opened".into())),
    }
}

/// Opens Android's setting letting Prisme install applications, for the DJ to allow it.
#[cfg(target_os = "android")]
#[tauri::command]
pub fn allow_installs() {
    call("open Android's setting for installing applications", |env, class| {
        env.call_static_method(class, "allow", "()V", &[])?.v()
    });
}

/// Runs `f` with `Update` ([`android_jni::call`]).
#[cfg(target_os = "android")]
fn call<T>(what: &str, f: impl FnOnce(&mut JNIEnv, &JClass) -> jni::errors::Result<T>) -> Option<T> {
    android_jni::call(what, |env, context| {
        let class = android_jni::class(env, context, CLASS)?;
        f(env, &class)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_apk_of_a_release_is_downloaded() {
        let release =
            "https://github.com/Shawiizz/prisme/releases/download/0.3.10/Prisme_0.3.10_android-arm64.apk";
        assert_eq!(release_apk(release), Some("Prisme_0.3.10_android-arm64.apk"));
        for url in [
            "https://github.com/Shawiizz/prisme/releases/download/0.3.10/Prisme_0.3.10_x64-setup.exe",
            "https://github.com/Shawiizz/prisme/releases/download/0.3.10/../../../x.apk",
            "https://github.com/Shawiizz/prisme/releases/download/../0.3.10/x.apk",
            "https://github.com/Shawiizz/prisme/releases/download/0.3.10/sub/x.apk",
            "https://github.com/Shawiizz/prisme/releases/download/0.3.10/.apk",
            "https://github.com/Shawiizz/prisme/releases/download//x.apk",
            "https://github.com/Other/prisme/releases/download/0.3.10/x.apk",
            "http://github.com/Shawiizz/prisme/releases/download/0.3.10/x.apk",
            "https://github.com/Shawiizz/prisme/releases/download/0.3.10/x.apk?a=b",
        ] {
            assert_eq!(release_apk(url), None, "{url}");
        }
    }
}
