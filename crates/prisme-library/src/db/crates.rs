//! Crates: named sets of tracks (as in Serato), shown sorted by the library columns.

use super::tracks::record_from_row;
use super::{path_key, DbError, LibraryDb, TrackRecord};
use rusqlite::{params, OptionalExtension};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CrateId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crate {
    pub id: CrateId,
    pub name: String,
    pub track_count: usize,
}

/// A track of a crate, with what the library knows of it (`None` if it was never
/// analysed).
#[derive(Debug, Clone, PartialEq)]
pub struct CrateEntry {
    pub path: String,
    pub record: Option<TrackRecord>,
}

impl LibraryDb {
    /// Every crate, by name.
    pub fn crates(&self) -> Result<Vec<Crate>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT crates.id, crates.name, COUNT(crate_tracks.path) FROM crates
             LEFT JOIN crate_tracks ON crate_tracks.crate_id = crates.id
             GROUP BY crates.id ORDER BY crates.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Crate { id: CrateId(r.get(0)?), name: r.get(1)?, track_count: r.get::<_, i64>(2)? as usize })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn create_crate(&self, name: &str) -> Result<CrateId, DbError> {
        let conn = self.conn();
        conn.prepare_cached("INSERT INTO crates (name) VALUES (?1)")?.execute([name])?;
        Ok(CrateId(conn.last_insert_rowid()))
    }

    pub fn rename_crate(&self, id: CrateId, name: &str) -> Result<(), DbError> {
        self.conn()
            .prepare_cached("UPDATE crates SET name = ?2 WHERE id = ?1")?
            .execute(params![id.0, name])?;
        Ok(())
    }

    /// Deletes the crate, not its tracks.
    pub fn delete_crate(&self, id: CrateId) -> Result<(), DbError> {
        self.conn().prepare_cached("DELETE FROM crates WHERE id = ?1")?.execute([id.0])?;
        Ok(())
    }

    /// Adds tracks to a crate (those already in it are left as they are); returns how
    /// many were added.
    pub fn add_to_crate(&self, id: CrateId, paths: &[&Path]) -> Result<usize, DbError> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let mut added = 0;
        {
            let mut insert = tx.prepare_cached(
                "INSERT OR IGNORE INTO crate_tracks (crate_id, path, added_at_ms) VALUES (?1, ?2, ?3)",
            )?;
            let now = super::now_ms();
            for path in paths {
                added += insert.execute(params![id.0, path_key(path), now])?;
            }
        }
        tx.commit()?;
        Ok(added)
    }

    pub fn remove_from_crate(&self, id: CrateId, paths: &[&Path]) -> Result<(), DbError> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        {
            let mut delete =
                tx.prepare_cached("DELETE FROM crate_tracks WHERE crate_id = ?1 AND path = ?2")?;
            for path in paths {
                delete.execute(params![id.0, path_key(path)])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// The tracks of a crate, in the order they were added.
    pub fn crate_tracks(&self, id: CrateId) -> Result<Vec<CrateEntry>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT crate_tracks.path AS entry_path, tracks.* FROM crate_tracks
             LEFT JOIN tracks ON tracks.path = crate_tracks.path
             WHERE crate_tracks.crate_id = ?1 ORDER BY crate_tracks.added_at_ms, crate_tracks.rowid",
        )?;
        let rows = stmt.query_map([id.0], |r| {
            let analysed = r.get::<_, Option<String>>("path")?.is_some();
            Ok(CrateEntry {
                path: r.get("entry_path")?,
                record: if analysed { Some(record_from_row(r)?) } else { None },
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn crate_name(&self, id: CrateId) -> Result<Option<String>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached("SELECT name FROM crates WHERE id = ?1")?;
        Ok(stmt.query_row([id.0], |r| r.get(0)).optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::FileStamp;

    fn analysed(db: &LibraryDb, path: &str) {
        let stamp = FileStamp { size: 1, modified_ms: 2 };
        db.save(&TrackRecord::failed(Path::new(path), stamp, "x".into())).unwrap();
    }

    #[test]
    fn crates_hold_tracks_without_duplicates() {
        let db = LibraryDb::open_in_memory().unwrap();
        let warmup = db.create_crate("Warm-up").unwrap();
        let peak = db.create_crate("peak time").unwrap();
        analysed(&db, "a.mp3");

        let added = db.add_to_crate(warmup, &[Path::new("a.mp3"), Path::new("b.mp3")]).unwrap();
        assert_eq!(added, 2);
        assert_eq!(db.add_to_crate(warmup, &[Path::new("a.mp3")]).unwrap(), 0);

        let entries = db.crate_tracks(warmup).unwrap();
        assert_eq!(entries.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), ["a.mp3", "b.mp3"]);
        assert!(entries[0].record.is_some(), "a.mp3 is in the library");
        assert!(entries[1].record.is_none(), "b.mp3 was never analysed");
        assert!(db.crate_tracks(peak).unwrap().is_empty());

        let names: Vec<_> = db.crates().unwrap().into_iter().map(|c| (c.name, c.track_count)).collect();
        assert_eq!(names, [("peak time".to_string(), 0), ("Warm-up".to_string(), 2)]);
    }

    #[test]
    fn removing_renaming_and_deleting() {
        let db = LibraryDb::open_in_memory().unwrap();
        let id = db.create_crate("Old").unwrap();
        db.add_to_crate(id, &[Path::new("a.mp3"), Path::new("b.mp3")]).unwrap();
        db.remove_from_crate(id, &[Path::new("a.mp3")]).unwrap();
        assert_eq!(db.crate_tracks(id).unwrap().len(), 1);

        db.rename_crate(id, "New").unwrap();
        assert_eq!(db.crate_name(id).unwrap().as_deref(), Some("New"));

        db.delete_crate(id).unwrap();
        assert!(db.crates().unwrap().is_empty());
        assert!(db.crate_tracks(id).unwrap().is_empty(), "its tracks went with it");
    }
}
