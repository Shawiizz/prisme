//! Finding the audio files of the music folders.

use crate::db::FileStamp;
use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

/// The copy a tag write makes beside its file lives for a moment: one untouched for this
/// long was left behind by a write cut short.
const TAG_COPY_LEFT_AFTER: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub stamp: FileStamp,
}

/// What a scan of a music folder found.
#[derive(Debug, Default)]
pub struct Scan {
    /// The decodable files, sorted by path.
    pub files: Vec<ScannedFile>,
    /// Decodable files there but kept online only by a cloud folder (OneDrive and the
    /// like): reading them would download them.
    pub online_only: Vec<PathBuf>,
    /// Folders that could not be listed (a drive unplugged, a share offline, no access):
    /// what they hold is unknown, not gone.
    pub unreadable: Vec<PathBuf>,
    /// Copies of files being tagged that writes cut short left behind (see
    /// [`prisme_media::is_tag_copy`]): of no use, their files being as they were.
    pub tag_copies: Vec<PathBuf>,
}

impl Scan {
    /// Whether `path` may still be there, unseen: under a folder that could not be listed.
    pub fn may_hold(&self, path: &Path) -> bool {
        self.unreadable.iter().any(|dir| path.starts_with(dir))
    }
}

/// Every decodable file under `root`, recursively. Hidden files, system folders (the
/// recycle bin) and symbolic links to folders are skipped (no loops), but for the copies tag
/// writes left behind. Stops early, with what it found so far, when `cancel` is set.
pub fn scan_folder(root: &Path, cancel: &AtomicBool) -> Scan {
    scan_folder_at(root, cancel, SystemTime::now())
}

/// [`scan_folder`] of each of `roots`, together: a file under two of them (one inside the
/// other) is found once.
pub fn scan_folders(roots: &[PathBuf], cancel: &AtomicBool) -> Scan {
    let mut all = Scan::default();
    for root in roots {
        let scan = scan_folder(root, cancel);
        all.files.extend(scan.files);
        all.online_only.extend(scan.online_only);
        all.unreadable.extend(scan.unreadable);
        all.tag_copies.extend(scan.tag_copies);
    }
    all.files.sort_by(|a, b| a.path.cmp(&b.path));
    all.files.dedup_by(|a, b| a.path == b.path);
    for paths in [&mut all.online_only, &mut all.tag_copies] {
        paths.sort();
        paths.dedup();
    }
    all
}

/// Whether `path` is `folder` or under it; on Windows, whatever the case of the letters.
pub fn within(path: &Path, folder: &Path) -> bool {
    let names = |path: &Path| -> Vec<String> {
        path.components()
            .map(|c| {
                let name = c.as_os_str().to_string_lossy();
                if cfg!(windows) {
                    name.to_lowercase()
                } else {
                    name.into_owned()
                }
            })
            .collect()
    };
    names(path).starts_with(&names(folder))
}

/// The deepest of the music folders `roots` that `path` is or is in (see [`within`]).
pub fn root_of<'r>(path: &Path, roots: &'r [PathBuf]) -> Option<&'r PathBuf> {
    roots.iter().filter(|root| within(path, root)).max_by_key(|root| root.components().count())
}

fn scan_folder_at(root: &Path, cancel: &AtomicBool, now: SystemTime) -> Scan {
    let mut scan = Scan::default();
    let mut folders = vec![root.to_path_buf()];
    while let Some(dir) = folders.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            scan.unreadable.push(dir);
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') {
                let left = |meta: Metadata| untouched_for(&meta, now) >= TAG_COPY_LEFT_AFTER;
                if prisme_media::is_tag_copy(&name) && entry.metadata().is_ok_and(left) {
                    scan.tag_copies.push(entry.path());
                }
                continue;
            }
            let (Ok(kind), Ok(meta)) = (entry.file_type(), entry.metadata()) else { continue };
            let path = entry.path();
            let attributes = Attributes::of(&meta);
            if kind.is_dir() {
                if !(attributes.hidden && attributes.system) {
                    folders.push(path);
                }
            } else if attributes.hidden {
                continue;
            } else if prisme_media::is_supported(&path) {
                if attributes.online_only {
                    scan.online_only.push(path);
                } else {
                    scan.files.push(ScannedFile { path, stamp: FileStamp::from_metadata(&meta) });
                }
            }
        }
    }
    scan.files.sort_by(|a, b| a.path.cmp(&b.path));
    scan
}

/// Whether the file at `path` is kept online only by a cloud folder: reading it would
/// download it.
pub(crate) fn is_online_only(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| Attributes::of(&meta).online_only)
}

/// How long the file of `meta` has gone unwritten at `now`: since it was created, or written
/// last, whichever came later (a copy may keep the time its original was written).
fn untouched_for(meta: &Metadata, now: SystemTime) -> Duration {
    let touched = [meta.created(), meta.modified()].into_iter().flatten().max();
    touched.and_then(|t| now.duration_since(t).ok()).unwrap_or_default()
}

/// What the file system says of an entry beyond its kind.
struct Attributes {
    hidden: bool,
    /// Belonging to the system (also set on folders given a custom icon, so alone it does
    /// not hide a folder).
    system: bool,
    /// A cloud placeholder whose content is not on the disk.
    online_only: bool,
}

impl Attributes {
    #[cfg(windows)]
    fn of(meta: &std::fs::Metadata) -> Self {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN: u32 = 0x2;
        const SYSTEM: u32 = 0x4;
        const OFFLINE: u32 = 0x1000;
        const RECALL_ON_OPEN: u32 = 0x4_0000;
        const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
        let a = meta.file_attributes();
        Self {
            hidden: a & HIDDEN != 0,
            system: a & SYSTEM != 0,
            online_only: a & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0,
        }
    }

    #[cfg(not(windows))]
    fn of(_: &std::fs::Metadata) -> Self {
        Self { hidden: false, system: false, online_only: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_is_within_the_deepest_music_folder_it_is_in() {
        let roots = [PathBuf::from("/music"), PathBuf::from("/music/sets"), PathBuf::from("/usb")];
        assert_eq!(root_of(Path::new("/music/sets/2024"), &roots), Some(&roots[1]));
        assert_eq!(root_of(Path::new("/music"), &roots), Some(&roots[0]));
        assert_eq!(root_of(Path::new("/musical"), &roots), None, "a name that starts the same");
        let windows = cfg!(windows).then_some(&roots[0]);
        assert_eq!(root_of(Path::new("/Music/x"), &roots), windows, "a case of its own on Windows");
    }

    #[test]
    fn finds_audio_files_in_sub_folders() {
        let root = std::env::temp_dir().join(format!("prisme-scan-test-{}", std::process::id()));
        std::fs::create_dir_all(root.join("House/Deep")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        for f in
            ["a.mp3", "House/b.FLAC", "House/Deep/c.wav", "House/cover.jpg", ".hidden/d.mp3", "notes.txt"]
        {
            std::fs::write(root.join(f), b"x").unwrap();
        }
        let found: Vec<String> = scan_folder(&root, &AtomicBool::new(false))
            .files
            .iter()
            .map(|f| f.path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(found, ["House/Deep/c.wav", "House/b.FLAC", "a.mp3"]);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn copies_left_behind_by_tag_writes_are_found_once_untouched_for_a_while() {
        let root = std::env::temp_dir().join(format!("prisme-scan-copies-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let copy = root.join(".a.mp3.prisme-tags");
        for f in [&copy, &root.join(".notes")] {
            std::fs::write(f, b"x").unwrap();
        }
        let cancel = AtomicBool::new(false);
        assert!(scan_folder_at(&root, &cancel, SystemTime::now()).tag_copies.is_empty(), "being written");
        let later = SystemTime::now() + 2 * TAG_COPY_LEFT_AFTER;
        let scan = scan_folder_at(&root, &cancel, later);
        assert_eq!(scan.tag_copies, [copy]);
        assert!(scan.files.is_empty());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_folder_that_cannot_be_listed_may_still_hold_its_files() {
        let root = std::env::temp_dir().join(format!("prisme-scan-missing-{}", std::process::id()));
        let scan = scan_folder(&root, &AtomicBool::new(false));
        assert!(scan.files.is_empty());
        assert!(scan.may_hold(&root.join("House/a.mp3")));
        assert!(!scan.may_hold(Path::new("C:/elsewhere/a.mp3")));
    }

    #[test]
    fn folders_compare_whatever_the_case_on_windows() {
        let (path, folder) = (Path::new("e:/sets/House"), Path::new("E:/Sets"));
        assert_eq!(within(path, folder), cfg!(windows));
        assert!(within(Path::new("E:/Sets/House"), folder));
        assert!(!within(Path::new("E:/Sets 2024/a.mp3"), folder), "a name that only starts the same");
    }

    #[test]
    fn folders_scanned_together_find_a_file_once() {
        let root = std::env::temp_dir().join(format!("prisme-scan-together-{}", std::process::id()));
        std::fs::create_dir_all(root.join("Music/House")).unwrap();
        std::fs::create_dir_all(root.join("Key")).unwrap();
        for f in ["Music/a.mp3", "Music/House/b.flac", "Key/c.wav"] {
            std::fs::write(root.join(f), b"x").unwrap();
        }
        let (music, key, missing) = (root.join("Music"), root.join("Key"), root.join("Unplugged"));
        let roots = [music.clone(), key, music.join("House"), missing.clone()];
        let scan = scan_folders(&roots, &AtomicBool::new(false));
        let found: Vec<String> = scan
            .files
            .iter()
            .map(|f| f.path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(found, ["Key/c.wav", "Music/House/b.flac", "Music/a.mp3"]);
        assert_eq!(scan.unreadable, [missing]);
        std::fs::remove_dir_all(root).ok();
    }
}
