//! Finding the audio files of a music folder.

use crate::db::FileStamp;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub stamp: FileStamp,
}

/// Every decodable file under `root`, recursively, sorted by path. Hidden entries
/// and symbolic links to folders are skipped (no loops). Stops early, with what it
/// found so far, when `cancel` is set.
pub fn scan_folder(root: &Path, cancel: &AtomicBool) -> Vec<ScannedFile> {
    let mut files = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(dir) = folders.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_dir() {
                folders.push(path);
            } else if prisme_media::is_supported(&path) {
                if let Ok(meta) = entry.metadata() {
                    files.push(ScannedFile { path, stamp: FileStamp::from_metadata(&meta) });
                }
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_audio_files_in_sub_folders() {
        let root = std::env::temp_dir().join(format!("prisme-scan-test-{}", std::process::id()));
        std::fs::create_dir_all(root.join("House/Deep")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        for f in ["a.mp3", "House/b.FLAC", "House/Deep/c.wav", "House/cover.jpg", ".hidden/d.mp3", "notes.txt"] {
            std::fs::write(root.join(f), b"x").unwrap();
        }
        let found: Vec<String> = scan_folder(&root, &AtomicBool::new(false))
            .iter()
            .map(|f| f.path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(found, ["House/Deep/c.wav", "House/b.FLAC", "a.mp3"]);
        std::fs::remove_dir_all(root).ok();
    }
}
