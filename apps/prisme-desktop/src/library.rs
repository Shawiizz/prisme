//! Browsing music folders.

use crate::dto::{DirEntryDto, DirListingDto};
use std::path::{Path, PathBuf};

/// Extensions the media decoder can play.
const AUDIO_EXTENSIONS: &[&str] = &["mp3", "flac", "wav", "m4a", "aac", "ogg", "oga", "mp4"];

/// The user's music folder (falls back to the home folder).
pub fn music_root() -> PathBuf {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from);
    match home {
        Some(home) if home.join("Music").is_dir() => home.join("Music"),
        Some(home) => home,
        None => PathBuf::from("."),
    }
}

pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Sub-folders and playable files of `dir`, sorted by name (hidden entries skipped).
pub fn list_dir(dir: &Path) -> std::io::Result<DirListingDto> {
    let mut folders = Vec::new();
    let mut tracks = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        let item = DirEntryDto { name, path: path.display().to_string(), size_bytes: meta.len() };
        if meta.is_dir() {
            folders.push(item);
        } else if is_audio_file(&path) {
            tracks.push(item);
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

/// Title shown for a file until tags are read: the file name without extension.
pub fn title_from_path(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_audio_files() {
        assert!(is_audio_file(Path::new("a/b/Track.MP3")));
        assert!(is_audio_file(Path::new("x.flac")));
        assert!(!is_audio_file(Path::new("cover.jpg")));
        assert!(!is_audio_file(Path::new("noext")));
    }

    #[test]
    fn title_is_the_file_stem() {
        assert_eq!(title_from_path(Path::new("C:/Music/House/Mi Casa - 2fox.mp3")), "Mi Casa - 2fox");
    }
}
