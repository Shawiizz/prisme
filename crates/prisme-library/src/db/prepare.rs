//! The prepare list: the tracks lined up for the next minutes of the set, in order.

use super::tracks::entry_from_row;
use super::{path_key, DbError, LibraryDb, TrackEntry};
use rusqlite::params;
use std::path::Path;

impl LibraryDb {
    pub fn prepare_list(&self) -> Result<Vec<TrackEntry>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT prepare.path AS entry_path, track_view.* FROM prepare
             LEFT JOIN track_view ON track_view.path = prepare.path ORDER BY prepare.position",
        )?;
        let rows = stmt.query_map([], entry_from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Appends tracks (those already in the list stay where they are); returns how many
    /// were added.
    pub fn add_to_prepare(&self, paths: &[&Path]) -> Result<usize, DbError> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let mut added = 0;
        {
            let mut insert = tx.prepare_cached(
                "INSERT OR IGNORE INTO prepare (path, position)
                 VALUES (?1, (SELECT COALESCE(MAX(position), 0) + 1 FROM prepare))",
            )?;
            for path in paths {
                added += insert.execute([path_key(path)])?;
            }
        }
        tx.commit()?;
        Ok(added)
    }

    /// Returns how many of the tracks were in the list.
    pub fn remove_from_prepare(&self, paths: &[&Path]) -> Result<usize, DbError> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let mut removed = 0;
        {
            let mut delete = tx.prepare_cached("DELETE FROM prepare WHERE path = ?1")?;
            for path in paths {
                removed += delete.execute([path_key(path)])?;
            }
        }
        tx.commit()?;
        Ok(removed)
    }

    /// Moves a track of the list to `index` (clamped to the list).
    pub fn move_in_prepare(&self, path: &Path, index: usize) -> Result<(), DbError> {
        let key = path_key(path);
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let mut order: Vec<String> = {
            let mut stmt = tx.prepare_cached("SELECT path FROM prepare ORDER BY position")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        let Some(from) = order.iter().position(|p| *p == key) else { return Ok(()) };
        let moved = order.remove(from);
        order.insert(index.min(order.len()), moved);
        {
            let mut update = tx.prepare_cached("UPDATE prepare SET position = ?2 WHERE path = ?1")?;
            for (position, p) in order.iter().enumerate() {
                update.execute(params![p, position as i64 + 1])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(db: &LibraryDb) -> Vec<String> {
        db.prepare_list().unwrap().into_iter().map(|e| e.path).collect()
    }

    #[test]
    fn tracks_are_appended_once_and_can_be_moved() {
        let db = LibraryDb::open_in_memory().unwrap();
        let paths = [Path::new("a.mp3"), Path::new("b.mp3"), Path::new("c.mp3")];
        assert_eq!(db.add_to_prepare(&paths).unwrap(), 3);
        assert_eq!(db.add_to_prepare(&[Path::new("a.mp3")]).unwrap(), 0);
        assert_eq!(order(&db), ["a.mp3", "b.mp3", "c.mp3"]);

        db.move_in_prepare(Path::new("c.mp3"), 0).unwrap();
        assert_eq!(order(&db), ["c.mp3", "a.mp3", "b.mp3"]);
        db.move_in_prepare(Path::new("c.mp3"), 99).unwrap();
        assert_eq!(order(&db), ["a.mp3", "b.mp3", "c.mp3"]);

        db.add_to_prepare(&[Path::new("d.mp3")]).unwrap();
        assert_eq!(order(&db), ["a.mp3", "b.mp3", "c.mp3", "d.mp3"]);
    }

    #[test]
    fn removing_reports_what_was_there() {
        let db = LibraryDb::open_in_memory().unwrap();
        db.add_to_prepare(&[Path::new("a.mp3"), Path::new("b.mp3")]).unwrap();
        assert_eq!(db.remove_from_prepare(&[Path::new("a.mp3"), Path::new("x.mp3")]).unwrap(), 1);
        assert_eq!(order(&db), ["b.mp3"]);
        assert_eq!(db.remove_from_prepare(&[Path::new("a.mp3")]).unwrap(), 0);
    }
}
