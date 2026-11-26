//! The prepare list: the tracks lined up for the next minutes of the set, in order.

use super::tracks::entry_from_row;
use super::{path_key, placed_before, DbError, LibraryDb, TrackEntry};
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

    /// Puts `paths` in the list before `before` (`None`: at its end), in their order, those not
    /// in it added: one change. Nothing moves when `before` is one of them.
    pub fn place_in_prepare(&self, paths: &[&Path], before: Option<&Path>) -> Result<(), DbError> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let order: Vec<String> = {
            let mut stmt = tx.prepare_cached("SELECT path FROM prepare ORDER BY position")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        let placed = paths.iter().map(|path| path_key(path)).collect();
        let Some(order) = placed_before(order, placed, before.map(path_key)) else {
            return Ok(());
        };
        tx.prepare_cached("DELETE FROM prepare")?.execute([])?;
        {
            let mut insert = tx.prepare_cached("INSERT INTO prepare (path, position) VALUES (?1, ?2)")?;
            for (position, path) in order.iter().enumerate() {
                insert.execute(params![path, position as i64 + 1])?;
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
    fn tracks_are_appended_once_and_placed_before_another_in_their_order() {
        let db = LibraryDb::open_in_memory().unwrap();
        let path = Path::new;
        assert_eq!(db.add_to_prepare(&[path("a.mp3"), path("b.mp3"), path("c.mp3")]).unwrap(), 3);
        assert_eq!(db.add_to_prepare(&[path("a.mp3")]).unwrap(), 0);
        assert_eq!(order(&db), ["a.mp3", "b.mp3", "c.mp3"]);

        db.place_in_prepare(&[path("c.mp3"), path("b.mp3")], Some(path("a.mp3"))).unwrap();
        assert_eq!(order(&db), ["c.mp3", "b.mp3", "a.mp3"]);
        db.place_in_prepare(&[path("c.mp3")], None).unwrap();
        assert_eq!(order(&db), ["b.mp3", "a.mp3", "c.mp3"]);
        db.place_in_prepare(&[path("d.mp3"), path("a.mp3")], Some(path("b.mp3"))).unwrap();
        assert_eq!(order(&db), ["d.mp3", "a.mp3", "b.mp3", "c.mp3"], "one added on the way");
        db.place_in_prepare(&[path("a.mp3"), path("b.mp3")], Some(path("b.mp3"))).unwrap();
        assert_eq!(order(&db), ["d.mp3", "a.mp3", "b.mp3", "c.mp3"], "before one of them: nothing moves");
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
