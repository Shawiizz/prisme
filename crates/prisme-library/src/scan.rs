//! Finding the audio files of a music folder.

use crate::db::FileStamp;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

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
}

impl Scan {
    /// Whether `path` may still be there, unseen: under a folder that could not be listed.
    pub fn may_hold(&self, path: &Path) -> bool {
        self.unreadable.iter().any(|dir| path.starts_with(dir))
    }
}

/// Every decodable file under `root`, recursively. Hidden files, system folders (the
/// recycle bin) and symbolic links to folders are skipped (no loops). Stops early, with what
/// it found so far, when `cancel` is set.
pub fn scan_folder(root: &Path, cancel: &AtomicBool) -> Scan {
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
            if entry.file_name().to_string_lossy().starts_with('.') {
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
    fn a_folder_that_cannot_be_listed_may_still_hold_its_files() {
        let root = std::env::temp_dir().join(format!("prisme-scan-missing-{}", std::process::id()));
        let scan = scan_folder(&root, &AtomicBool::new(false));
        assert!(scan.files.is_empty());
        assert!(scan.may_hold(&root.join("House/a.mp3")));
        assert!(!scan.may_hold(Path::new("C:/elsewhere/a.mp3")));
    }
}
