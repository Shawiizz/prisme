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
