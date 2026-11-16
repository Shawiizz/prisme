//! Browsing music folders.

use crate::dto::{DirEntryDto, DirListingDto, TrackDetailsDto};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// The music folder: the one the DJ `chosen`, else the system's. A phone keeps the
/// system's: Android shares the music of applications there, and iOS lets Prisme read its
/// own documents only.
pub fn music_root(chosen: Option<&str>) -> PathBuf {
    if cfg!(target_os = "android") {
        return system_music_root();
    }
    crate::dev_startup::music_dir()
        .or_else(|| chosen.filter(|_| cfg!(desktop)).map(PathBuf::from))
        .unwrap_or_else(system_music_root)
}

/// The system's music folder: where Windows keeps it (moved to another drive, a NAS or
/// OneDrive as well), else `Music` in the home folder. Never the home folder itself, whose
/// scan would read the whole profile. Not looked at: a NAS offline would hold the start for
/// as long as the network waits. On a phone: the music folder Android shares between
/// applications, and on iOS the documents of Prisme, which the Files application fills.
pub fn system_music_root() -> PathBuf {
    #[cfg(target_os = "android")]
    return android::music_root().clone();
    #[cfg(not(target_os = "android"))]
    {
        let home =
            |folder: &str| dirs::home_dir().map_or_else(|| PathBuf::from(folder), |home| home.join(folder));
        if cfg!(target_os = "ios") {
            return home("Documents");
        }
        dirs::audio_dir().unwrap_or_else(|| home("Music"))
    }
}

#[cfg(target_os = "android")]
mod android {
    use crate::android_jni;
    use jni::objects::{JString, JValue};
    use std::path::PathBuf;
    use std::sync::OnceLock;

    /// Where Android usually keeps it, if it cannot be asked.
    const USUAL: &str = "/storage/emulated/0/Music";

    /// The music folder Android shares, asked once.
    pub fn music_root() -> &'static PathBuf {
        static ROOT: OnceLock<PathBuf> = OnceLock::new();
        ROOT.get_or_init(|| asked().unwrap_or_else(|| PathBuf::from(USUAL)))
    }

    /// `Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_MUSIC)`.
    fn asked() -> Option<PathBuf> {
        android_jni::call("find the music folder", |env, _| {
            const ENVIRONMENT: &str = "android/os/Environment";
            let music = env.get_static_field(ENVIRONMENT, "DIRECTORY_MUSIC", "Ljava/lang/String;")?.l()?;
            let folder = env
                .call_static_method(
                    ENVIRONMENT,
                    "getExternalStoragePublicDirectory",
                    "(Ljava/lang/String;)Ljava/io/File;",
                    &[JValue::Object(&music)],
                )?
                .l()?;
            if folder.is_null() {
                return Ok(None);
            }
            let path =
                JString::from(env.call_method(&folder, "getAbsolutePath", "()Ljava/lang/String;", &[])?.l()?);
            let path: String = env.get_string(&path)?.into();
            Ok(Some(PathBuf::from(path)))
        })
        .flatten()
    }
}

/// Sub-folders and playable files of `dir`, sorted by name (hidden entries skipped).
/// `details` gives what the library knows of a track, if anything. The music folder `root`
/// of a system that has none yet is empty.
pub fn list_dir(
    dir: &Path,
    root: &Path,
    details: impl Fn(&Path, &std::fs::Metadata) -> Option<TrackDetailsDto>,
) -> std::io::Result<DirListingDto> {
    let mut folders = Vec::new();
    let mut tracks = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Err(e) if e.kind() == ErrorKind::NotFound && dir == root => None,
        entries => Some(entries?),
    };
    for entry in entries.into_iter().flatten() {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        let item = |details| DirEntryDto { name, path: path.display().to_string(), details };
        if meta.is_dir() {
            folders.push(item(None));
        } else if prisme_media::is_supported(&path) {
            tracks.push(item(details(&path, &meta)));
        }
    }
    let by_name = |a: &DirEntryDto, b: &DirEntryDto| a.name.to_lowercase().cmp(&b.name.to_lowercase());
    folders.sort_by(by_name);
    tracks.sort_by(by_name);
    Ok(DirListingDto {
        path: dir.display().to_string(),
        parent: dir.parent().map(|p| p.display().to_string()),
        folders,
        tracks,
    })
}

/// `to`, where the folder `from` was moved if it existed and `to` did not; `from` if it could
/// not move (another drive, a file in use).
pub fn moved(from: &Path, to: PathBuf) -> PathBuf {
    if from.is_dir() && !to.exists() {
        let moved =
            to.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::rename(from, &to));
        if let Err(e) = moved {
            tracing::warn!("{} not moved to {}: {e}", from.display(), to.display());
            return from.to_owned();
        }
    }
    to
}

/// Title shown for a file until tags are read: the file name without extension.
pub fn title_from_path(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_moves_where_nothing_is_yet() {
        let root = std::env::temp_dir().join(format!("prisme-moved-{}", std::process::id()));
        let (from, to) = (root.join("roaming/models"), root.join("local/models"));
        std::fs::create_dir_all(&from).unwrap();
        std::fs::write(from.join("model"), b"weights").unwrap();
        assert_eq!(moved(&from, to.clone()), to);
        assert!(!from.exists() && to.join("model").is_file());
        std::fs::create_dir_all(&from).unwrap();
        assert_eq!(moved(&from, to.clone()), to, "the folder already there stays");
        assert!(from.is_dir());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_music_folder_chosen_replaces_the_systems_on_a_computer() {
        if crate::dev_startup::music_dir().is_some() {
            return;
        }
        assert_eq!(music_root(None), system_music_root());
        let chosen = music_root(Some("D:/Sets"));
        assert_eq!(chosen == Path::new("D:/Sets"), cfg!(desktop));
    }

    #[test]
    fn title_is_the_file_stem() {
        assert_eq!(title_from_path(Path::new("C:/Music/House/Mi Casa - 2fox.mp3")), "Mi Casa - 2fox");
    }
}
