//! Browsing music folders.

use crate::dto::{DirEntryDto, DirListingDto, TrackDetailsDto};
use std::path::{Path, PathBuf};

/// The user's music folder (falls back to the home folder).
pub fn music_root() -> PathBuf {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from);
    match home {
        Some(home) if home.join("Music").is_dir() => home.join("Music"),
        Some(home) => home,
        None => PathBuf::from("."),
    }
}

/// Sub-folders and playable files of `dir`, sorted by name (hidden entries skipped).
/// `details` gives what the library knows of a track, if anything.
pub fn list_dir(
    dir: &Path,
    details: impl Fn(&Path, &std::fs::Metadata) -> Option<TrackDetailsDto>,
) -> std::io::Result<DirListingDto> {
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

/// Title shown for a file until tags are read: the file name without extension.
pub fn title_from_path(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_is_the_file_stem() {
        assert_eq!(title_from_path(Path::new("C:/Music/House/Mi Casa - 2fox.mp3")), "Mi Casa - 2fox");
    }
}
