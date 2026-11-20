//! Browsing music folders.

use crate::dto::{StoragePlaceDto, StoragePlaceKind};
use prisme_library::FileStamp;
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
pub use android::shared_documents;

#[cfg(target_os = "android")]
mod android {
    use crate::android_jni;
    use jni::objects::{JString, JValue};
    use std::path::PathBuf;
    use std::sync::OnceLock;

    /// Where Android usually keeps them, if it cannot be asked.
    const MUSIC: &str = "/storage/emulated/0/Music";
    const DOCUMENTS: &str = "/storage/emulated/0/Documents";

    /// The music folder Android shares, asked once.
    pub fn music_root() -> &'static PathBuf {
        static ROOT: OnceLock<PathBuf> = OnceLock::new();
        ROOT.get_or_init(|| shared("DIRECTORY_MUSIC").unwrap_or_else(|| PathBuf::from(MUSIC)))
    }

    /// The documents folder Android shares between applications, where recordings go: Prisme's
    /// own is hidden from the Files application, and deleted with Prisme.
    pub fn shared_documents() -> PathBuf {
        shared("DIRECTORY_DOCUMENTS").unwrap_or_else(|| PathBuf::from(DOCUMENTS))
    }

    /// `Environment.getExternalStoragePublicDirectory(Environment.<directory>)`.
    fn shared(directory: &str) -> Option<PathBuf> {
        android_jni::call("find a shared folder", |env, _| {
            const ENVIRONMENT: &str = "android/os/Environment";
            let name = env.get_static_field(ENVIRONMENT, directory, "Ljava/lang/String;")?.l()?;
            let folder = env
                .call_static_method(
                    ENVIRONMENT,
                    "getExternalStoragePublicDirectory",
                    "(Ljava/lang/String;)Ljava/io/File;",
                    &[JValue::Object(&name)],
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

/// Where Android mounts the phone's own storage (`emulated`), each memory card and USB drive
/// beside it.
const ANDROID_STORAGE: &str = "/storage";
/// The phone's own storage, if its music folder does not say.
const ANDROID_PHONE: &str = "/storage/emulated/0";

/// Where a folder to add is looked for first. On Android: the phone's own storage, the one
/// its music folder is in, then each memory card and USB drive that can be read. On a
/// computer: the home folder, then the drives.
pub fn storage_places() -> Vec<StoragePlaceDto> {
    if cfg!(target_os = "android") {
        let music = system_music_root();
        let phone = music.parent().map_or_else(|| PathBuf::from(ANDROID_PHONE), Path::to_path_buf);
        return android_places(Path::new(ANDROID_STORAGE), phone);
    }
    let home = dirs::home_dir().map(|home| place(StoragePlaceKind::Home, home));
    let drives: Vec<PathBuf> = if cfg!(windows) {
        (b'A'..=b'Z')
            .map(|letter| PathBuf::from(format!("{}:\\", char::from(letter))))
            .filter(|drive| drive.is_dir())
            .collect()
    } else {
        vec![PathBuf::from("/")]
    };
    home.into_iter().chain(drives.into_iter().map(|drive| place(StoragePlaceKind::Drive, drive))).collect()
}

/// `phone`, then the volumes in `storage` that can be read, by name: not the phone's storage
/// seen from there (`emulated`, `self`).
fn android_places(storage: &Path, phone: PathBuf) -> Vec<StoragePlaceDto> {
    let mut volumes: Vec<PathBuf> = std::fs::read_dir(storage)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            !(name.starts_with('.') || name == "emulated" || name == "self")
        })
        .map(|entry| entry.path())
        .filter(|volume| std::fs::read_dir(volume).is_ok())
        .collect();
    volumes.sort();
    std::iter::once(place(StoragePlaceKind::Phone, phone))
        .chain(volumes.into_iter().map(|volume| place(StoragePlaceKind::Removable, volume)))
        .collect()
}

/// `path` named by its last folder: a drive by its letter (`C:`), the root of all as itself.
fn place(kind: StoragePlaceKind, path: PathBuf) -> StoragePlaceDto {
    let path = path.display().to_string();
    let last = path.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or_default();
    let name = if last.is_empty() { path.clone() } else { last.to_owned() };
    StoragePlaceDto { kind, name, path }
}

/// The folders in a folder and its playable files, each by name, the hidden ones left out,
/// each file with the version of it on the disk.
pub struct Listing {
    pub folders: Vec<PathBuf>,
    pub files: Vec<(PathBuf, FileStamp)>,
}

/// What `dir` holds. One of the music folders `roots` that is not there (a system that has none
/// yet, a drive unplugged) holds nothing.
pub fn listing(dir: &Path, roots: &[PathBuf]) -> std::io::Result<Listing> {
    let mut folders = Vec::new();
    let mut files = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Err(e) if e.kind() == ErrorKind::NotFound && roots.iter().any(|root| root == dir) => None,
        entries => Some(entries?),
    };
    for entry in entries.into_iter().flatten() {
        let Ok(entry) = entry else { continue };
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            folders.push(path);
        } else if prisme_media::is_supported(&path) {
            files.push((path, FileStamp::from_metadata(&meta)));
        }
    }
    let name = |path: &Path| path.file_name().map(|n| n.to_string_lossy().to_lowercase());
    folders.sort_by_cached_key(|path| name(path));
    files.sort_by_cached_key(|(path, _)| name(path));
    Ok(Listing { folders, files })
}

/// Shows `path` in the system's file manager: a folder opened, a file selected in its own.
#[cfg(desktop)]
pub fn show(path: &Path) -> std::io::Result<()> {
    use std::process::Command;
    let file = path.is_file();
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut explorer = Command::new("explorer");
        if file {
            // as Explorer reads it: the path quoted after the comma
            explorer.raw_arg(format!("/select,\"{}\"", path.display()));
        } else {
            explorer.arg(path);
        }
        explorer.spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        let mut open = Command::new("open");
        if file {
            open.arg("-R");
        }
        open.arg(path).spawn()?;
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let folder = if file { path.parent().unwrap_or(path) } else { path };
        Command::new("xdg-open").arg(folder).spawn()?;
    }
    Ok(())
}

/// A phone shows no folders.
#[cfg(mobile)]
pub fn show(_path: &Path) -> std::io::Result<()> {
    Err(std::io::ErrorKind::Unsupported.into())
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
    fn android_offers_the_phone_then_each_volume_it_can_read() {
        let storage = std::env::temp_dir().join(format!("prisme-storage-{}", std::process::id()));
        for volume in ["emulated/0", "self/primary", "3A2F-1C0B", "0000-0001", ".hidden"] {
            std::fs::create_dir_all(storage.join(volume)).unwrap();
        }
        std::fs::write(storage.join("not-a-volume"), b"").unwrap();
        let places = android_places(&storage, storage.join("emulated/0"));
        let named: Vec<_> = places.iter().map(|p| (p.kind, p.name.as_str())).collect();
        assert_eq!(
            named,
            [
                (StoragePlaceKind::Phone, "0"),
                (StoragePlaceKind::Removable, "0000-0001"),
                (StoragePlaceKind::Removable, "3A2F-1C0B"),
            ]
        );
        assert_eq!(places[2].path, storage.join("3A2F-1C0B").display().to_string());
        std::fs::remove_dir_all(storage).unwrap();
    }

    #[test]
    fn a_place_is_named_by_its_last_folder_or_as_a_whole() {
        let name = |path: &str| place(StoragePlaceKind::Drive, PathBuf::from(path)).name;
        assert_eq!(name("/storage/3A2F-1C0B"), "3A2F-1C0B");
        assert_eq!(name("C:\\"), "C:");
        assert_eq!(name("/"), "/");
    }

    #[test]
    fn title_is_the_file_stem() {
        assert_eq!(title_from_path(Path::new("C:/Music/House/Mi Casa - 2fox.mp3")), "Mi Casa - 2fox");
    }
}
