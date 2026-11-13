//! What tells a file again once it is elsewhere: the identity of its audio
//! ([`prisme_media::audio_id`]), kept by path even after the track is forgotten, as its
//! cues and corrections are; and its size and time.

use super::{path_key, DbError, FileStamp, LibraryDb};
use rusqlite::{params, OptionalExtension};
use std::collections::HashMap;
use std::path::Path;

impl LibraryDb {
    /// The file of `path` sounds like `id`.
    pub fn set_audio_id(&self, path: &Path, id: u64) -> Result<(), DbError> {
        self.conn()
            .prepare_cached("INSERT OR REPLACE INTO audio_ids (path, audio_id) VALUES (?1, ?2)")?
            .execute(params![path_key(path), id as i64])?;
        Ok(())
    }

    pub fn audio_id(&self, path: &Path) -> Result<Option<u64>, DbError> {
        let id: Option<i64> = self
            .conn()
            .prepare_cached("SELECT audio_id FROM audio_ids WHERE path = ?1")?
            .query_row([path_key(path)], |r| r.get(0))
            .optional()?;
        Ok(id.map(|id| id as u64))
    }

    /// Every path whose audio is known, with its identity.
    pub fn audio_ids(&self) -> Result<HashMap<String, u64>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT path, audio_id FROM audio_ids")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The paths whose file sounded like `id`.
    pub fn paths_sounding_like(&self, id: u64) -> Result<Vec<String>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached("SELECT path FROM audio_ids WHERE audio_id = ?1")?;
        let rows = stmt.query_map([id as i64], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The tracks stored from a file of this size and time.
    pub fn paths_stamped(&self, stamp: FileStamp) -> Result<Vec<String>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached("SELECT path FROM tracks WHERE size = ?1 AND modified_ms = ?2")?;
        let rows = stmt.query_map(params![stamp.size as i64, stamp.modified_ms], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The stored tracks whose audio has no identity yet.
    pub fn paths_without_audio_id(&self) -> Result<Vec<String>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT path FROM tracks WHERE error IS NULL AND path NOT IN (SELECT path FROM audio_ids)",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Whether a track is stored at `path`.
    pub fn has_track(&self, path: &Path) -> Result<bool, DbError> {
        Ok(self.conn().prepare_cached("SELECT 1 FROM tracks WHERE path = ?1")?.exists([path_key(path)])?)
    }
}

#[cfg(test)]
mod tests {
    use crate::db::{FileStamp, LibraryDb, TrackRecord};
    use crate::{Cue, CueSlot};
    use prisme_analysis::TrackAnalysis;
    use prisme_media::TrackTags;
    use std::path::Path;

    fn track(db: &LibraryDb, path: &Path, audio: u64) {
        let stamp = FileStamp { size: 1, modified_ms: 1 };
        db.save(&TrackRecord::analyzed(path, stamp, 1.0, TrackAnalysis::default(), TrackTags::default()))
            .unwrap();
        db.set_audio_id(path, audio).unwrap();
    }

    #[test]
    fn a_forgotten_file_is_known_by_its_audio_while_it_leaves_something() {
        let db = LibraryDb::open_in_memory().unwrap();
        let (cued, bare) = (Path::new("cued.mp3"), Path::new("bare.mp3"));
        track(&db, cued, 7);
        track(&db, bare, 7);
        db.save_cue(cued, &Cue::point(CueSlot::Hot(0), 1.0)).unwrap();
        db.forget(&["cued.mp3".into(), "bare.mp3".into()]).unwrap();
        assert_eq!(db.paths_sounding_like(7).unwrap(), ["cued.mp3"]);
        assert!(db.paths_without_audio_id().unwrap().is_empty());
    }

    #[test]
    fn what_another_file_left_at_a_path_does_not_mix_with_the_file_moved_there() {
        let db = LibraryDb::open_in_memory().unwrap();
        let (from, to) = (Path::new("old/track.mp3"), Path::new("new/track.mp3"));
        // another file was at the new path, and left a cue there
        track(&db, to, 1);
        db.save_cue(to, &Cue::point(CueSlot::Hot(0), 99.0)).unwrap();
        db.forget(&["new/track.mp3".into()]).unwrap();
        track(&db, from, 2);
        db.save_cue(from, &Cue::point(CueSlot::Hot(0), 3.0)).unwrap();

        db.relocate(from, to).unwrap();
        let cues = db.cues(to).unwrap();
        assert_eq!(cues.iter().map(|c| c.position_s).collect::<Vec<_>>(), [3.0]);
        assert_eq!(db.audio_id(to).unwrap(), Some(2));
        assert_eq!(db.audio_id(from).unwrap(), None);
    }

    #[test]
    fn the_same_file_met_again_at_its_path_keeps_what_was_there() {
        let db = LibraryDb::open_in_memory().unwrap();
        let (from, to) = (Path::new("old/track.mp3"), Path::new("new/track.mp3"));
        track(&db, to, 5);
        db.save_cue(to, &Cue::point(CueSlot::Hot(1), 8.0)).unwrap();
        db.forget(&["new/track.mp3".into()]).unwrap();
        track(&db, from, 5);
        db.save_cue(from, &Cue::point(CueSlot::Hot(0), 3.0)).unwrap();

        db.relocate(from, to).unwrap();
        assert_eq!(db.cues(to).unwrap().len(), 2);
    }
}
