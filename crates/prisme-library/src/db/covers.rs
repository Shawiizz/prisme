use super::{path_key, DbError, LibraryDb};
use crate::covers;
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

    /// The cover thumbnail of the track at `path`: its file's, else one drawn for it; `None` for
    /// a file not in the library.
    pub fn cover(&self, path: &Path) -> Result<Option<Vec<u8>>, DbError> {
        if let Some(jpeg) = self.stored_cover(path)? {
            return Ok(Some(jpeg));
        }
        Ok(self.get(path)?.map(|track| covers::drawn(&track.tags, path)))
    }

    fn stored_cover(&self, path: &Path) -> Result<Option<Vec<u8>>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached("SELECT jpeg FROM covers WHERE path = ?1")?;
        Ok(stmt.query_row([path_key(path)], |r| r.get(0)).optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::catalogue::tests::tagged;

    #[test]
    fn covers_are_stored_per_track() {
        let db = LibraryDb::open_in_memory().unwrap();
        db.save_cover(Path::new("a.mp3"), &[1, 2, 3]).unwrap();
        db.save_cover(Path::new("a.mp3"), &[4, 5]).unwrap();
        assert_eq!(db.cover(Path::new("a.mp3")).unwrap(), Some(vec![4, 5]));
    }

    #[test]
    fn a_track_without_a_cover_has_one_drawn_a_file_not_in_the_library_none() {
        let db = LibraryDb::open_in_memory().unwrap();
        let track = tagged("b.mp3", "Night Ferry", "Mira Solenne");
        db.save(&track).unwrap();
        assert_eq!(
            db.cover(Path::new("b.mp3")).unwrap(),
            Some(covers::drawn(&track.tags, Path::new("b.mp3")))
        );
        assert_eq!(db.cover(Path::new("c.mp3")).unwrap(), None);
    }
}
