//! Import from other DJ software: crates (playlists), cue points, saved loops, beat grids,
//! colours and ratings.
//!
//! * [`serato`]: crates of the `_Serato_` folder, markers in the files' tags;
//! * [`rekordbox`]: the collection exported as XML.
//!
//! [`apply`] merges an import into the library without overwriting anything: what was
//! already set in Prisme stays, so importing again changes nothing.

pub mod rekordbox;
pub mod serato;

use prisme_library::{Cue, CueSlot, DbError, GridEdit, LibraryDb, HOT_CUES};
use std::path::{Path, PathBuf};

/// Cues closer than this are the same cue (positions are stored to the millisecond).
const SAME_PLACE_S: f64 = 0.002;

#[derive(Debug, Default, PartialEq)]
pub struct Import {
    pub tracks: Vec<ImportedTrack>,
    pub crates: Vec<ImportedCrate>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportedTrack {
    pub path: PathBuf,
    pub cues: Vec<ImportedCue>,
    pub grid: Option<ImportedGrid>,
    /// 0xRRGGBB
    pub color: Option<u32>,
    /// 1 to 5 stars.
    pub rating: Option<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportedCue {
    /// The hot cue; `None` for a memory cue (becomes the main cue) or a saved loop
    /// without a slot (takes a free hot cue).
    pub hot: Option<u8>,
    pub position_s: f64,
    pub loop_length_s: Option<f64>,
    /// 0xRRGGBB; 0 for none.
    pub color: u32,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImportedGrid {
    pub bpm: f64,
    /// A beat (any one).
    pub beat_s: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportedCrate {
    pub name: String,
    pub paths: Vec<PathBuf>,
}

/// What an import brought.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportSummary {
    /// Tracks that got something (cues, grid, colour or rating).
    pub tracks: usize,
    pub cues: usize,
    pub grids: usize,
    /// Crates created or completed.
    pub crates: usize,
    /// Files named by the import that are not on this computer.
    pub missing: usize,
}

/// Merges `import` into the library; see the module documentation.
pub fn apply(db: &LibraryDb, import: &Import) -> Result<ImportSummary, DbError> {
    let mut summary = ImportSummary::default();
    for track in &import.tracks {
        if !track.path.exists() {
            summary.missing += 1;
            continue;
        }
        let cues = apply_cues(db, &track.path, &track.cues)?;
        let grid = apply_grid(db, &track.path, track.grid)?;
        let marked = apply_marks(db, &track.path, track.color, track.rating)?;
        summary.cues += cues;
        summary.grids += grid as usize;
        summary.tracks += (cues > 0 || grid || marked) as usize;
    }
    let existing = db.crates()?;
    for imported in &import.crates {
        let paths: Vec<&Path> = imported.paths.iter().map(PathBuf::as_path).filter(|p| p.exists()).collect();
        summary.missing += imported.paths.len() - paths.len();
        let id = match existing.iter().find(|c| c.name == imported.name && c.rules.is_none()) {
            Some(c) => c.id,
            None => db.create_crate(&imported.name)?,
        };
        db.add_to_crate(id, &paths)?;
        summary.crates += 1;
    }
    Ok(summary)
}

/// Hot cues go to their slot if it is free; saved loops without a slot to the first free
/// one; the first memory cue becomes the main cue if there is none. A cue already at the
/// same place is not added again.
fn apply_cues(db: &LibraryDb, path: &Path, cues: &[ImportedCue]) -> Result<usize, DbError> {
    let mut existing = db.cues(path)?;
    let mut added = 0;
    for imported in cues {
        let same_place = |c: &Cue| {
            (c.position_s - imported.position_s).abs() < SAME_PLACE_S
                && c.loop_length_s.is_some() == imported.loop_length_s.is_some()
        };
        if existing.iter().any(same_place) {
            continue;
        }
        let taken: Vec<CueSlot> = existing.iter().map(|c| c.slot).collect();
        let free_hot = || (0..HOT_CUES).map(CueSlot::Hot).find(|s| !taken.contains(s));
        let slot = match (imported.hot, imported.loop_length_s) {
            (Some(n), _) if n < HOT_CUES => Some(CueSlot::Hot(n)).filter(|s| !taken.contains(s)),
            (Some(_), _) => None,
            (None, Some(_)) => free_hot(),
            (None, None) => Some(CueSlot::Main).filter(|s| !taken.contains(s)),
        };
        let Some(slot) = slot else { continue };
        let mut cue = match imported.loop_length_s {
            Some(length) => Cue::looped(slot, imported.position_s, length),
            None => Cue::point(slot, imported.position_s),
        };
        if imported.color != 0 {
            cue.color = imported.color;
        }
        cue.label = imported.label.clone();
        db.save_cue(path, &cue)?;
        existing.push(cue);
        added += 1;
    }
    Ok(added)
}

fn apply_grid(db: &LibraryDb, path: &Path, grid: Option<ImportedGrid>) -> Result<bool, DbError> {
    let Some(grid) = grid.filter(|g| GridEdit::BPM_RANGE.contains(&g.bpm)) else { return Ok(false) };
    let mut edits = db.edits(path)?;
    if edits.grid.is_some() {
        return Ok(false);
    }
    edits.grid = Some(GridEdit { bpm: grid.bpm, first_beat_s: 0.0 }.with_beat_at(grid.beat_s));
    db.save_edits(path, &edits)?;
    Ok(true)
}

fn apply_marks(db: &LibraryDb, path: &Path, color: Option<u32>, rating: Option<u8>) -> Result<bool, DbError> {
    let marks = db.marks(path)?;
    let color = color.filter(|_| marks.color.is_none());
    let rating = rating.filter(|_| marks.rating == 0);
    if let Some(color) = color {
        db.set_color(&[path], Some(color))?;
    }
    if let Some(rating) = rating {
        db.set_rating(&[path], rating)?;
    }
    Ok(color.is_some() || rating.is_some())
}

/// `base` joined with a `/`-separated relative path, with the platform's separators (the
/// form the library stores paths in).
pub(crate) fn join_relative(base: &Path, relative: &str) -> PathBuf {
    relative.split('/').filter(|part| !part.is_empty()).fold(base.to_path_buf(), |path, part| path.join(part))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("prisme-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, b"x").unwrap();
        path
    }

    fn cue(hot: Option<u8>, position_s: f64, loop_length_s: Option<f64>) -> ImportedCue {
        ImportedCue { hot, position_s, loop_length_s, color: 0x112233, label: "x".into() }
    }

    #[test]
    fn an_import_fills_the_gaps_and_keeps_what_is_set() {
        let db = LibraryDb::open_in_memory().unwrap();
        let path = temp_file("a.mp3");
        db.save_cue(&path, &Cue::point(CueSlot::Hot(0), 10.0)).unwrap();
        db.set_rating(&[&path], 2).unwrap();
        let import = Import {
            tracks: vec![
                ImportedTrack {
                    path: path.clone(),
                    cues: vec![
                        cue(Some(0), 1.0, None),
                        cue(Some(1), 2.0, None),
                        cue(None, 3.0, Some(4.0)),
                        cue(None, 0.5, None),
                        cue(None, 0.7, None),
                    ],
                    grid: Some(ImportedGrid { bpm: 125.0, beat_s: 12.25 }),
                    color: Some(0xff0000),
                    rating: Some(5),
                },
                ImportedTrack {
                    path: PathBuf::from("Z:/nowhere.mp3"),
                    cues: vec![],
                    grid: None,
                    color: None,
                    rating: None,
                },
            ],
            crates: vec![ImportedCrate {
                name: "Imported".into(),
                paths: vec![path.clone(), PathBuf::from("Z:/x.mp3")],
            }],
        };

        let summary = apply(&db, &import).unwrap();
        assert_eq!(summary, ImportSummary { tracks: 1, cues: 3, grids: 1, crates: 1, missing: 2 });
        let cues = db.cues(&path).unwrap();
        let slots: Vec<_> = cues.iter().map(|c| (c.slot, c.position_s, c.loop_length_s)).collect();
        assert_eq!(
            slots,
            [
                (CueSlot::Main, 0.5, None),
                (CueSlot::Hot(0), 10.0, None),
                (CueSlot::Hot(1), 2.0, None),
                (CueSlot::Hot(2), 3.0, Some(4.0))
            ]
        );
        assert_eq!(cues[2].color, 0x112233);
        let grid = db.edits(&path).unwrap().grid.unwrap();
        assert!((grid.bpm, (grid.first_beat_s - 0.25).abs() < 1e-9) == (125.0, true));
        assert_eq!(db.marks(&path).unwrap(), prisme_library::TrackMarks { color: Some(0xff0000), rating: 2 });

        let again = apply(&db, &import).unwrap();
        assert_eq!((again.cues, again.grids, again.tracks), (0, 0, 0), "nothing changes the second time");
        assert_eq!(db.crates().unwrap().len(), 1);
        assert_eq!(db.crate_tracks(db.crates().unwrap()[0].id).unwrap().len(), 1);
    }

    #[test]
    fn relative_paths_take_the_platform_separators() {
        assert_eq!(
            join_relative(Path::new("C:\\"), "Users/Me//a.mp3"),
            Path::new("C:\\").join("Users").join("Me").join("a.mp3")
        );
    }
}
