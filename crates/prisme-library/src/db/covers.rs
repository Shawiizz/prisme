use super::{path_key, DbError, LibraryDb};
use rusqlite::{params, OptionalExtension};
use std::path::Path;

impl LibraryDb {
    /// Stores a track's cover thumbnail (JPEG).
    pub fn save_cover(&self, path: &Path, jpeg: &[u8]) -> Result<(), DbError> {
        self.conn()
            .prepare_cached("INSERT OR REPLACE INTO covers (path, jpeg) VALUES (?1, ?2)")?
            .execute(params![path_key(path), jpeg])?;
        Ok(())
    }

    pub fn cover(&self, path: &Path) -> Result<Option<Vec<u8>>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached("SELECT jpeg FROM covers WHERE path = ?1")?;
        Ok(stmt.query_row([path_key(path)], |r| r.get(0)).optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_are_stored_per_track() {
        let db = LibraryDb::open_in_memory().unwrap();
        db.save_cover(Path::new("a.mp3"), &[1, 2, 3]).unwrap();
        db.save_cover(Path::new("a.mp3"), &[4, 5]).unwrap();
        assert_eq!(db.cover(Path::new("a.mp3")).unwrap(), Some(vec![4, 5]));
        assert_eq!(db.cover(Path::new("b.mp3")).unwrap(), None);
    }
}
