//! Which tracks still need their tempo and key written into their file's tags (a track
//! needs it again once it is analysed again or corrected by hand), what Prisme wrote there
//! (its own to replace, when another program's is kept), and the records that follow a
//! file whose tags Prisme wrote.

use super::tracks::record_from_row;
use super::{path_key, DbError, FileStamp, LibraryDb, TrackRecord};
use prisme_media::TempoAndKey;
use rusqlite::{params, OptionalExtension, ToSql};
use std::path::Path;

/// The tempo and key Prisme wrote into a file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WrittenTags {
    pub bpm: Option<f64>,
    pub key: Option<String>,
}

impl WrittenTags {
    pub fn values(&self) -> TempoAndKey<'_> {
        TempoAndKey { bpm: self.bpm, key: self.key.as_deref() }
    }
}

impl LibraryDb {
    /// The analysed tracks whose file does not hold their current tempo and key.
    pub fn tracks_to_tag(&self) -> Result<Vec<TrackRecord>, DbError> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare_cached("SELECT * FROM track_view WHERE tags_written = 0 AND error IS NULL")?;
        let rows = stmt.query_map([], record_from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The file of `path` now holds the track's tempo and key (or another program's, kept).
    /// Writing them took the file from `before` to `after`: what was known of `before`
    /// follows, so that the change is not taken for a new version of the file. Nothing
    /// changes if the record no longer describes `before` (the file changed meanwhile and
    /// will be analysed again).
    pub fn tags_written(&self, path: &Path, before: FileStamp, after: FileStamp) -> Result<(), DbError> {
        self.restamp(path, before, after, "tags_written", &1)
    }

    /// Prisme wrote `values` into the file of `path`; one not given stays as recorded.
    pub fn own_tags_written(&self, path: &Path, values: TempoAndKey<'_>) -> Result<(), DbError> {
        self.conn()
            .prepare_cached(
                "UPDATE tracks SET tag_bpm = COALESCE(?2, tag_bpm), tag_key = COALESCE(?3, tag_key)
                 WHERE path = ?1",
            )?
            .execute(params![path_key(path), values.bpm, values.key])?;
        Ok(())
    }

    /// What Prisme wrote into the file of `path`, as far as it knows.
    pub fn own_tags(&self, path: &Path) -> Result<WrittenTags, DbError> {
        let own = self
            .conn()
            .prepare_cached("SELECT tag_bpm, tag_key FROM tracks WHERE path = ?1")?
            .query_row([path_key(path)], |r| Ok(WrittenTags { bpm: r.get(0)?, key: r.get(1)? }))
            .optional()?;
        Ok(own.unwrap_or_default())
    }

    /// Every analysed track is to be written again (the tempos and keys of other programs
    /// may now be replaced).
    pub fn retag_all(&self) -> Result<(), DbError> {
        self.conn().execute("UPDATE tracks SET tags_written = 0 WHERE tags_written = 1", [])?;
        Ok(())
    }

    /// The file of `path` went from `before` to `after` as its tempo and key were written,
    /// but they were corrected meanwhile: it is to be written again, not analysed again.
    pub fn tags_outdated(&self, path: &Path, before: FileStamp, after: FileStamp) -> Result<(), DbError> {
        self.restamp(path, before, after, "tags_written", &0)
    }

    /// The file of `path` now holds `genre` (`None`: none), written from `before` to
    /// `after`; as with [`LibraryDb::tags_written`], the change is not a new version.
    pub fn genre_written(
        &self,
        path: &Path,
        genre: Option<&str>,
        before: FileStamp,
        after: FileStamp,
    ) -> Result<(), DbError> {
        self.restamp(path, before, after, "genre", &genre)
    }

    /// Moves the record of `path` (and its waveform) from `before` to `after`, setting
    /// `column` to `value`.
    fn restamp(
        &self,
        path: &Path,
        before: FileStamp,
        after: FileStamp,
        column: &str,
        value: &dyn ToSql,
    ) -> Result<(), DbError> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let stamps = params![
            path_key(path),
            before.size as i64,
            before.modified_ms,
            after.size as i64,
            after.modified_ms
        ];
        tx.prepare_cached(&format!(
            "UPDATE tracks SET {column} = ?6, size = ?4, modified_ms = ?5
             WHERE path = ?1 AND size = ?2 AND modified_ms = ?3"
        ))?
        .execute([stamps, &[value]].concat().as_slice())?;
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
    use crate::db::{GridEdit, TrackEdits, MIGRATIONS};
    use prisme_analysis::{Key, KeyEstimate, Mode, Tempo, TrackAnalysis};
    use prisme_media::TrackTags;

    const BEFORE: FileStamp = FileStamp { size: 1, modified_ms: 1 };
    const AFTER: FileStamp = FileStamp { size: 2, modified_ms: 2 };

    fn save(db: &LibraryDb, path: &str, stamp: FileStamp, analysis: TrackAnalysis) {
        db.save(&TrackRecord::analyzed(Path::new(path), stamp, 1.0, analysis, TrackTags::default())).unwrap();
    }

    fn to_tag(db: &LibraryDb) -> Vec<(String, FileStamp)> {
        db.tracks_to_tag().unwrap().into_iter().map(|r| (r.path, r.version.stamp)).collect()
    }

    #[test]
    fn a_file_written_follows_its_new_stamp_and_waits_again_if_outdated() {
        let db = LibraryDb::open_in_memory().unwrap();
        let (a, b) = (Path::new("a.mp3"), Path::new("b.mp3"));
        save(&db, "a.mp3", BEFORE, TrackAnalysis::default());
        save(&db, "b.mp3", BEFORE, TrackAnalysis::default());
        db.tags_written(a, BEFORE, AFTER).unwrap();
        db.tags_outdated(b, BEFORE, AFTER).unwrap();
        assert_eq!(to_tag(&db), [("b.mp3".to_owned(), AFTER)]);
        assert_eq!(db.get(a).unwrap().unwrap().version.stamp, AFTER);
        db.retag_all().unwrap();
        assert_eq!(to_tag(&db).len(), 2);
    }

    #[test]
    fn what_prisme_wrote_is_remembered_through_a_new_analysis() {
        let db = LibraryDb::open_in_memory().unwrap();
        let path = Path::new("a.mp3");
        assert_eq!(db.own_tags(path).unwrap(), WrittenTags::default());
        save(&db, "a.mp3", BEFORE, TrackAnalysis::default());
        db.own_tags_written(path, TempoAndKey { bpm: Some(124.0), key: None }).unwrap();
        db.own_tags_written(path, TempoAndKey { bpm: None, key: Some("Am") }).unwrap();
        // the file changed (another program tagged it): analysed again
        save(&db, "a.mp3", AFTER, TrackAnalysis::default());
        assert_eq!(db.own_tags(path).unwrap(), WrittenTags { bpm: Some(124.0), key: Some("Am".into()) });
    }

    #[test]
    fn the_tags_written_before_prisme_kept_track_are_its_own() {
        let db = LibraryDb::open_in_memory().unwrap();
        let keys = (0..12).flat_map(|t| [Key::new(t, Mode::Major), Key::new(t, Mode::Minor)]);
        for (i, key) in keys.clone().enumerate() {
            let analysis = TrackAnalysis {
                tempo: Some(Tempo { bpm: 120.0 + i as f64, first_beat_s: 0.0, confidence: 1.0 }),
                key: Some(KeyEstimate { key, confidence: 1.0, tuning_cents: 0.0 }),
            };
            save(&db, &format!("{i}.mp3"), BEFORE, analysis);
            db.tags_written(Path::new(&format!("{i}.mp3")), BEFORE, BEFORE).unwrap();
        }
        save(&db, "corrected.mp3", BEFORE, TrackAnalysis::default());
        let edits = TrackEdits {
            grid: Some(GridEdit { bpm: 87.5, first_beat_s: 0.1 }),
            key: Some(Key::new(6, Mode::Minor)),
        };
        db.save_edits(Path::new("corrected.mp3"), &edits).unwrap();
        db.tags_written(Path::new("corrected.mp3"), BEFORE, BEFORE).unwrap();
        save(&db, "unwritten.mp3", BEFORE, TrackAnalysis::default());

        let backfill = MIGRATIONS.last().unwrap().split_once("UPDATE").unwrap().1;
        db.conn().execute(&format!("UPDATE{backfill}"), []).unwrap();

        for (i, key) in keys.enumerate() {
            let own = db.own_tags(Path::new(&format!("{i}.mp3"))).unwrap();
            assert_eq!(own, WrittenTags { bpm: Some(120.0 + i as f64), key: Some(key.name().into()) });
        }
        let corrected = db.own_tags(Path::new("corrected.mp3")).unwrap();
        assert_eq!(corrected, WrittenTags { bpm: Some(87.5), key: Some("F#m".into()) });
        assert_eq!(db.own_tags(Path::new("unwritten.mp3")).unwrap(), WrittenTags::default());
    }
}
