//! Serato DJ: crates from the `_Serato_` folder, cues, loops, colours and grids from the
//! tags of the files.

mod crates;
mod markers;
mod tags;

pub use crates::read_crates;
pub use tags::read_serato_tags;

use crate::{Import, ImportedTrack};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The crates of `serato_dir` (if given), and the Serato tags of `files` and of the
/// tracks of those crates.
pub fn read(serato_dir: Option<&Path>, files: &[PathBuf]) -> std::io::Result<Import> {
    let crates = serato_dir.map(read_crates).transpose()?.unwrap_or_default();
    let mut all: BTreeSet<&PathBuf> = files.iter().collect();
    all.extend(crates.iter().flat_map(|c| &c.paths));
    let tracks = all
        .into_iter()
        .filter_map(|path| {
            let tags = read_serato_tags(path)?;
            let markers = tags.markers.unwrap_or_default();
            Some(ImportedTrack {
                path: path.clone(),
                cues: markers.cues,
                grid: tags.grid,
                color: markers.color,
                rating: None,
            })
        })
        .collect();
    Ok(Import { tracks, crates })
}
