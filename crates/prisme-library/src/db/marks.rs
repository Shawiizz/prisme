//! What the DJ marks tracks with: a colour and a rating.

use super::{path_key, DbError, LibraryDb};
use rusqlite::{params, OptionalExtension, Row};
use std::path::Path;

pub const MAX_RATING: u8 = 5;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TrackMarks {
    /// 0xRRGGBB
    pub color: Option<u32>,
    /// 0 (not rated) to [`MAX_RATING`].
    pub rating: u8,
}

impl TrackMarks {
    /// From the `mark_*` columns of `track_view`.
    pub(super) fn from_row(r: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self { color: r.get("mark_color")?, rating: r.get("mark_rating")? })
    }
}

impl LibraryDb {
    pub fn marks(&self, path: &Path) -> Result<TrackMarks, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT color AS mark_color, rating AS mark_rating FROM track_marks WHERE path = ?1",
        )?;
        Ok(stmt.query_row([path_key(path)], TrackMarks::from_row).optional()?.unwrap_or_default())
    }

    pub fn set_color(&self, paths: &[&Path], color: Option<u32>) -> Result<(), DbError> {
        self.mark(
            paths,
            "INSERT INTO track_marks (path, color) VALUES (?1, ?2)
             ON CONFLICT (path) DO UPDATE SET color = excluded.color",
            color,
        )
    }

    /// `rating` is capped at [`MAX_RATING`].
    pub fn set_rating(&self, paths: &[&Path], rating: u8) -> Result<(), DbError> {
        self.mark(
            paths,
            "INSERT INTO track_marks (path, rating) VALUES (?1, ?2)
             ON CONFLICT (path) DO UPDATE SET rating = excluded.rating",
            rating.min(MAX_RATING),
        )
    }

    fn mark(&self, paths: &[&Path], upsert: &str, value: impl rusqlite::ToSql) -> Result<(), DbError> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        {
            let mut set = tx.prepare_cached(upsert)?;
            for path in paths {
                set.execute(params![path_key(path), value])?;
            }
        }
        tx.execute("DELETE FROM track_marks WHERE color IS NULL AND rating = 0", [])?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{FileStamp, TrackRecord};

    #[test]
    fn marks_and_play_counts_come_with_the_record() {
        let db = LibraryDb::open_in_memory().unwrap();
        let path = Path::new("a.mp3");
        db.save(&TrackRecord::failed(path, FileStamp { size: 1, modified_ms: 1 }, "x".into())).unwrap();
        assert_eq!(db.get(path).unwrap().unwrap().marks, TrackMarks::default());

        db.set_color(&[path], Some(0xff4d6d)).unwrap();
        db.set_rating(&[path], 9).unwrap();
        let session = db.start_history_session(0).unwrap();
        db.record_play(session, path, 0, 1).unwrap();
        db.record_play(session, path, 1, 2).unwrap();
        let record = db.get(path).unwrap().unwrap();
        assert_eq!(record.marks, TrackMarks { color: Some(0xff4d6d), rating: MAX_RATING });
        assert_eq!(record.play_count, 2);

        db.set_color(&[path], None).unwrap();
        db.set_rating(&[path], 0).unwrap();
        assert_eq!(db.get(path).unwrap().unwrap().marks, TrackMarks::default());
    }
}
