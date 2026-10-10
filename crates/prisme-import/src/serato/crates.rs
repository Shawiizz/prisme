//! Serato crates: `_Serato_/Subcrates/*.crate`. A crate file is a list of blocks, each a
//! four-letter tag, a big-endian `u32` length and the content; each `otrk` block holds a
//! `ptrk` block with a track's path, in UTF-16BE, relative to the root of the drive the
//! `_Serato_` folder is on. Sub-crates are named `Parent%%Child.crate`.

use crate::ImportedCrate;
use std::path::{Component, Path, PathBuf};

/// The crates of a `_Serato_` folder, sorted by name.
pub fn read_crates(serato_dir: &Path) -> std::io::Result<Vec<ImportedCrate>> {
    let root = drive_root(serato_dir);
    let mut crates = Vec::new();
    for entry in std::fs::read_dir(serato_dir.join("Subcrates"))? {
        let path = entry?.path();
        if path.extension().is_none_or(|e| !e.eq_ignore_ascii_case("crate")) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        let paths =
            track_paths(&std::fs::read(&path)?).iter().map(|p| crate::join_relative(&root, p)).collect();
        crates.push(ImportedCrate { name: stem.replace("%%", " / "), paths });
    }
    crates.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(crates)
}

/// The tracks of a crate file, as stored (relative paths with `/`).
pub fn track_paths(data: &[u8]) -> Vec<String> {
    blocks(data)
        .filter(|(tag, _)| tag == b"otrk")
        .filter_map(|(_, track)| blocks(track).find(|(tag, _)| tag == b"ptrk"))
        .map(|(_, path)| utf16_be(path))
        .collect()
}

fn blocks(mut data: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8])> {
    std::iter::from_fn(move || {
        let tag: [u8; 4] = data.get(..4)?.try_into().ok()?;
        let length = u32::from_be_bytes(data.get(4..8)?.try_into().ok()?) as usize;
        let content = data.get(8..8 + length)?;
        data = &data[8 + length..];
        Some((tag, content))
    })
}

fn utf16_be(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&units)
}

/// `C:\` for `C:\Users\Me\Music\_Serato_`.
fn drive_root(path: &Path) -> PathBuf {
    path.components().take_while(|c| matches!(c, Component::Prefix(_) | Component::RootDir)).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn block(tag: &[u8; 4], content: &[u8]) -> Vec<u8> {
        let mut b = tag.to_vec();
        b.extend((content.len() as u32).to_be_bytes());
        b.extend(content);
        b
    }

    pub fn crate_file(paths: &[&str]) -> Vec<u8> {
        let utf16 = |s: &str| s.encode_utf16().flat_map(u16::to_be_bytes).collect::<Vec<u8>>();
        let mut data = block(b"vrsn", &utf16("1.0/Serato ScratchLive Crate"));
        data.extend(block(b"osrt", &block(b"tvcn", &utf16("song"))));
        for path in paths {
            data.extend(block(b"otrk", &block(b"ptrk", &utf16(path))));
        }
        data
    }

    #[test]
    fn tracks_of_a_crate_file() {
        let data = crate_file(&["Users/Me/Music/House/Café.mp3", "Users/Me/Music/b.flac"]);
        assert_eq!(track_paths(&data), ["Users/Me/Music/House/Café.mp3", "Users/Me/Music/b.flac"]);
        assert!(track_paths(b"short").is_empty());
    }

    #[test]
    fn crates_of_a_serato_folder() {
        let dir = std::env::temp_dir().join(format!("prisme-serato-{}", std::process::id()));
        let serato = dir.join("_Serato_");
        std::fs::create_dir_all(serato.join("Subcrates")).unwrap();
        std::fs::write(serato.join("Subcrates/House%%Deep.crate"), crate_file(&["Music/a.mp3"])).unwrap();
        std::fs::write(serato.join("Subcrates/Afro.crate"), crate_file(&[])).unwrap();
        std::fs::write(serato.join("Subcrates/notes.txt"), b"x").unwrap();

        let crates = read_crates(&serato).unwrap();
        assert_eq!(crates.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Afro", "House / Deep"]);
        assert_eq!(crates[1].paths, [drive_root(&serato).join("Music").join("a.mp3")]);
        std::fs::remove_dir_all(dir).ok();
    }
}
