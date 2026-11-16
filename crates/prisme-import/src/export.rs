//! What an export to other DJ software carries: tracks with what the library knows of them
//! and their cues, and lists of them in folders.

use crate::FOLDER_SEPARATOR;
use prisme_library::{Cue, DbError, LibraryDb, TrackEntry, TrackRecord};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Export {
    /// Every track the lists name, once.
    pub tracks: Vec<ExportedTrack>,
    pub lists: Vec<ExportedList>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportedTrack {
    pub path: PathBuf,
    /// `None` for a track the library never analysed.
    pub record: Option<TrackRecord>,
    pub cues: Vec<Cue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportedList {
    /// The folders the list is in, then its name.
    pub path: Vec<String>,
    /// In order; a track may come back.
    pub tracks: Vec<PathBuf>,
}

/// Where a list named `name` in `folder` goes: a name with ` / ` is in folders, as an
/// import names the playlists it finds in folders.
pub fn list_path(folder: Option<&str>, name: &str) -> Vec<String> {
    folder.into_iter().chain(name.split(FOLDER_SEPARATOR)).map(str::to_owned).collect()
}

impl Export {
    /// The lists (each its path and its tracks) with their tracks and those tracks' cues;
    /// with `whole_library`, every analysed track of the library as well, first.
    pub fn gather(
        db: &LibraryDb,
        whole_library: bool,
        lists: Vec<(Vec<String>, Vec<TrackEntry>)>,
    ) -> Result<Self, DbError> {
        let mut entries: Vec<TrackEntry> = Vec::new();
        if whole_library {
            let mut records = db.analysed_records()?;
            records.sort_by(|a, b| a.path.cmp(&b.path));
            entries.extend(records.into_iter().map(|r| TrackEntry { path: r.path.clone(), record: Some(r) }));
        }
        let mut export = Export::default();
        for (path, list_entries) in lists {
            let tracks = list_entries.iter().map(|e| PathBuf::from(&e.path)).collect();
            export.lists.push(ExportedList { path, tracks });
            entries.extend(list_entries);
        }
        let mut seen = HashSet::new();
        for entry in entries {
            if !seen.insert(entry.path.clone()) {
                continue;
            }
            let path = PathBuf::from(entry.path);
            let cues = db.cues(&path)?;
            export.tracks.push(ExportedTrack { path, record: entry.record, cues });
        }
        Ok(export)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prisme_library::{CueSlot, FileStamp};
    use std::path::Path;

    fn entry(path: &str) -> TrackEntry {
        TrackEntry { path: path.into(), record: None }
    }

    #[test]
    fn a_name_with_slashes_is_in_folders() {
        assert_eq!(
            list_path(Some("History"), "Festivals / Main stage"),
            ["History", "Festivals", "Main stage"]
        );
        assert_eq!(list_path(None, "Warm-up"), ["Warm-up"]);
    }

    #[test]
    fn each_track_comes_once_with_its_cues() {
        let db = LibraryDb::open_in_memory().unwrap();
        let stamp = FileStamp { size: 1, modified_ms: 2 };
        let analysis = prisme_analysis::TrackAnalysis { tempo: None, key: None };
        for path in ["z.mp3", "b.mp3"] {
            let record = TrackRecord::analyzed(Path::new(path), stamp, 100.0, analysis, Default::default());
            db.save(&record).unwrap();
        }
        db.save_cue(Path::new("x.mp3"), &Cue::point(CueSlot::Hot(3), 1.0)).unwrap();
        let lists = vec![
            (list_path(None, "A"), vec![entry("x.mp3"), entry("b.mp3")]),
            (list_path(None, "B"), vec![entry("x.mp3"), entry("x.mp3")]),
        ];

        let only_lists = Export::gather(&db, false, lists.clone()).unwrap();
        let paths: Vec<_> = only_lists.tracks.iter().map(|t| t.path.to_str().unwrap()).collect();
        assert_eq!(paths, ["x.mp3", "b.mp3"]);
        assert_eq!(only_lists.tracks[0].cues.len(), 1);
        assert!(only_lists.tracks[0].record.is_none());
        assert_eq!(only_lists.lists[1].tracks, [PathBuf::from("x.mp3"), PathBuf::from("x.mp3")]);

        let library = Export::gather(&db, true, lists).unwrap();
        let paths: Vec<_> = library.tracks.iter().map(|t| t.path.to_str().unwrap()).collect();
        assert_eq!(paths, ["b.mp3", "z.mp3", "x.mp3"]);
        assert!(library.tracks[0].record.is_some());
    }
}
