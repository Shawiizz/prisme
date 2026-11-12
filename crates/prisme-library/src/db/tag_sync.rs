//! Which tracks still need their tempo and key written into their file's tags (a track
//! needs it again once it is analysed again or corrected by hand), and the records that
//! follow a file whose tags Prisme wrote.

use super::tracks::record_from_row;
use super::{path_key, DbError, FileStamp, LibraryDb, TrackRecord};
use rusqlite::{params, ToSql};
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
        self.restamp(path, before, after, "tags_written", &1)
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
    use crate::db::{FileStamp, LibraryDb, TrackRecord};
    use prisme_analysis::TrackAnalysis;
    use prisme_media::TrackTags;
    use std::path::Path;

    fn to_tag(db: &LibraryDb) -> Vec<(String, FileStamp)> {
        db.tracks_to_tag().unwrap().into_iter().map(|r| (r.path, r.version.stamp)).collect()
    }

    #[test]
    fn a_file_written_follows_its_new_stamp_and_waits_again_if_outdated() {
        let db = LibraryDb::open_in_memory().unwrap();
        let (a, b) = (Path::new("a.mp3"), Path::new("b.mp3"));
        let (before, after) = (FileStamp { size: 1, modified_ms: 1 }, FileStamp { size: 2, modified_ms: 2 });
        for path in [a, b] {
            db.save(&TrackRecord::analyzed(
                path,
                before,
                1.0,
                TrackAnalysis::default(),
                TrackTags::default(),
            ))
            .unwrap();
        }
        db.tags_written(a, before, after).unwrap();
        db.tags_outdated(b, before, after).unwrap();
        assert_eq!(to_tag(&db), [("b.mp3".to_owned(), after)]);
        assert_eq!(db.get(a).unwrap().unwrap().version.stamp, after);
    }
}
