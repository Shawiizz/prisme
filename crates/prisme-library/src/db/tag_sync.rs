//! Which tracks still need their tempo and key written into their file's tags. A track
//! needs it again once it is analysed again or corrected by hand.

use super::tracks::record_from_row;
use super::{path_key, DbError, FileStamp, LibraryDb, TrackRecord};
use rusqlite::params;
use std::path::Path;

impl LibraryDb {
    /// The analysed tracks whose file does not hold their current tempo and key.
    pub fn tracks_to_tag(&self) -> Result<Vec<TrackRecord>, DbError> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare_cached("SELECT * FROM track_view WHERE tags_written = 0 AND error IS NULL")?;
        let rows = stmt.query_map([], record_from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The file of `path` now holds the track's tempo and key. Writing them took the file
    /// from `before` to `after`: what was known of `before` follows, so that the change
    /// is not taken for a new version of the file. Nothing changes if the record no
    /// longer describes `before` (the file changed meanwhile and will be analysed again).
    pub fn tags_written(&self, path: &Path, before: FileStamp, after: FileStamp) -> Result<(), DbError> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let stamps = params![
            path_key(path),
            before.size as i64,
            before.modified_ms,
            after.size as i64,
            after.modified_ms
        ];
        tx.prepare_cached(
            "UPDATE tracks SET tags_written = 1, size = ?4, modified_ms = ?5
             WHERE path = ?1 AND size = ?2 AND modified_ms = ?3",
        )?
        .execute(stamps)?;
        tx.prepare_cached(
            "UPDATE waveforms SET size = ?4, modified_ms = ?5 WHERE path = ?1 AND size = ?2 AND modified_ms = ?3",
        )?
        .execute(stamps)?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TrackEdits;
    use prisme_analysis::{Tempo, TrackAnalysis, Waveform};
    use prisme_media::TrackTags;

    const BEFORE: FileStamp = FileStamp { size: 100, modified_ms: 1 };
    const AFTER: FileStamp = FileStamp { size: 140, modified_ms: 2 };

    fn analysed(db: &LibraryDb, path: &str) {
        let tempo = Tempo { bpm: 124.0, first_beat_s: 0.1, confidence: 0.9 };
        let analysis = TrackAnalysis { tempo: Some(tempo), key: None };
        db.save(&TrackRecord::analyzed(Path::new(path), BEFORE, 300.0, analysis, TrackTags::default()))
            .unwrap();
    }

    fn to_tag(db: &LibraryDb) -> Vec<String> {
        db.tracks_to_tag().unwrap().into_iter().map(|r| r.path).collect()
    }

    #[test]
    fn written_tags_follow_the_file() {
        let db = LibraryDb::open_in_memory().unwrap();
        analysed(&db, "a.mp3");
        let waveform =
            Waveform { slices_per_second: 1, peak: vec![1], low: vec![1], mid: vec![1], high: vec![1] };
        db.save_waveform(Path::new("a.mp3"), BEFORE, &waveform).unwrap();
        assert_eq!(to_tag(&db), ["a.mp3"]);

        db.tags_written(Path::new("a.mp3"), BEFORE, AFTER).unwrap();
        assert!(to_tag(&db).is_empty());
        assert!(db.versions().unwrap()["a.mp3"].is_current(AFTER), "not a new version of the file");
        assert!(db.waveform(Path::new("a.mp3"), AFTER).unwrap().is_some());
    }

    #[test]
    fn a_file_changed_meanwhile_is_left_to_the_analysis() {
        let db = LibraryDb::open_in_memory().unwrap();
        analysed(&db, "a.mp3");
        db.tags_written(Path::new("a.mp3"), FileStamp { size: 999, modified_ms: 9 }, AFTER).unwrap();
        assert_eq!(to_tag(&db), ["a.mp3"]);
        assert!(db.versions().unwrap()["a.mp3"].is_current(BEFORE));
    }

    #[test]
    fn corrections_and_new_analyses_are_written_again() {
        let db = LibraryDb::open_in_memory().unwrap();
        analysed(&db, "a.mp3");
        analysed(&db, "b.mp3");
        db.tags_written(Path::new("a.mp3"), BEFORE, AFTER).unwrap();
        db.tags_written(Path::new("b.mp3"), BEFORE, AFTER).unwrap();

        db.save_edits(Path::new("a.mp3"), &TrackEdits::default()).unwrap();
        analysed(&db, "b.mp3");
        assert_eq!(to_tag(&db), ["a.mp3", "b.mp3"]);
    }

    #[test]
    fn failed_files_are_not_tagged() {
        let db = LibraryDb::open_in_memory().unwrap();
        db.save(&TrackRecord::failed(Path::new("bad.mp3"), BEFORE, "corrupt".into())).unwrap();
        assert!(to_tag(&db).is_empty());
    }
}
