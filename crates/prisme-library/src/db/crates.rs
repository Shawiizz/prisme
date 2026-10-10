//! Crates: named sets of tracks (as in Serato), shown sorted by the library columns. A
//! smart crate holds no tracks of its own: its rules pick them from the library.

use super::tracks::entry_from_row;
use super::{path_key, DbError, LibraryDb, TrackEntry};
use crate::smart::SmartRules;
use rusqlite::{params, OptionalExtension};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CrateId(pub i64);

#[derive(Debug, Clone, PartialEq)]
pub struct Crate {
    pub id: CrateId,
    pub name: String,
    pub rules: Option<SmartRules>,
    pub track_count: usize,
}

impl LibraryDb {
    /// Every crate, by name.
    pub fn crates(&self) -> Result<Vec<Crate>, DbError> {
        let crates: Vec<(CrateId, String, Option<String>, usize)> = {
            let conn = self.conn();
            let mut stmt = conn.prepare_cached(
                "SELECT crates.id, crates.name, crates.rules, COUNT(crate_tracks.path) FROM crates
                 LEFT JOIN crate_tracks ON crate_tracks.crate_id = crates.id
                 GROUP BY crates.id ORDER BY crates.name COLLATE NOCASE",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((CrateId(r.get(0)?), r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? as usize))
            })?;
            rows.collect::<Result<_, _>>()?
        };
        let smart = crates.iter().any(|c| c.2.is_some());
        let library = if smart { self.analysed_records()? } else { Vec::new() };
        crates
            .into_iter()
            .map(|(id, name, rules, count)| {
                let rules = rules.map(|json| serde_json::from_str::<SmartRules>(&json)).transpose()?;
                let track_count = match &rules {
                    Some(rules) => library.iter().filter(|r| rules.matches(r)).count(),
                    None => count,
                };
                Ok(Crate { id, name, rules, track_count })
            })
            .collect()
    }

    pub fn create_crate(&self, name: &str) -> Result<CrateId, DbError> {
        let conn = self.conn();
        conn.prepare_cached("INSERT INTO crates (name) VALUES (?1)")?.execute([name])?;
        Ok(CrateId(conn.last_insert_rowid()))
    }

    pub fn create_smart_crate(&self, name: &str, rules: &SmartRules) -> Result<CrateId, DbError> {
        let conn = self.conn();
        conn.prepare_cached("INSERT INTO crates (name, rules) VALUES (?1, ?2)")?
            .execute(params![name, serde_json::to_string(rules)?])?;
        Ok(CrateId(conn.last_insert_rowid()))
    }

    pub fn rename_crate(&self, id: CrateId, name: &str) -> Result<(), DbError> {
        self.conn()
            .prepare_cached("UPDATE crates SET name = ?2 WHERE id = ?1")?
            .execute(params![id.0, name])?;
        Ok(())
    }

    pub fn set_crate_rules(&self, id: CrateId, rules: &SmartRules) -> Result<(), DbError> {
        self.conn()
            .prepare_cached("UPDATE crates SET rules = ?2 WHERE id = ?1")?
            .execute(params![id.0, serde_json::to_string(rules)?])?;
        Ok(())
    }

    /// The rules of a smart crate; `None` for a crate filled by hand.
    pub fn crate_rules(&self, id: CrateId) -> Result<Option<SmartRules>, DbError> {
        let json: Option<String> = {
            let conn = self.conn();
            let mut stmt = conn.prepare_cached("SELECT rules FROM crates WHERE id = ?1")?;
            stmt.query_row([id.0], |r| r.get(0)).optional()?.flatten()
        };
        Ok(json.map(|j| serde_json::from_str(&j)).transpose()?)
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

    /// The tracks of a crate: in the order they were added, or those its rules pick.
    pub fn crate_tracks(&self, id: CrateId) -> Result<Vec<TrackEntry>, DbError> {
        if let Some(rules) = self.crate_rules(id)? {
            let library = self.analysed_records()?;
            return Ok(library
                .into_iter()
                .filter(|r| rules.matches(r))
                .map(|r| TrackEntry { path: r.path.clone(), record: Some(r) })
                .collect());
        }
        let conn = self.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT crate_tracks.path AS entry_path, track_view.* FROM crate_tracks
             LEFT JOIN track_view ON track_view.path = crate_tracks.path
             WHERE crate_tracks.crate_id = ?1 ORDER BY crate_tracks.added_at_ms, crate_tracks.rowid",
        )?;
        let rows = stmt.query_map([id.0], entry_from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{FileStamp, TrackRecord};
    use crate::smart::{Rule, TextField};
    use prisme_analysis::TrackAnalysis;
    use prisme_media::TrackTags;

    fn failed(db: &LibraryDb, path: &str) {
        let stamp = FileStamp { size: 1, modified_ms: 2 };
        db.save(&TrackRecord::failed(Path::new(path), stamp, "x".into())).unwrap();
    }

    fn analysed(db: &LibraryDb, path: &str, genre: &str) {
        let tags = TrackTags { genre: Some(genre.into()), ..TrackTags::default() };
        let analysis = TrackAnalysis { tempo: None, key: None };
        let stamp = FileStamp { size: 1, modified_ms: 2 };
        db.save(&TrackRecord::analyzed(Path::new(path), stamp, 1.0, analysis, tags)).unwrap();
    }

    #[test]
    fn crates_hold_tracks_without_duplicates() {
        let db = LibraryDb::open_in_memory().unwrap();
        let warmup = db.create_crate("Warm-up").unwrap();
        let peak = db.create_crate("peak time").unwrap();
        failed(&db, "a.mp3");

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
        assert_eq!(db.crates().unwrap()[0].name, "New");

        db.delete_crate(id).unwrap();
        assert!(db.crates().unwrap().is_empty());
        assert!(db.crate_tracks(id).unwrap().is_empty(), "its tracks went with it");
    }

    #[test]
    fn smart_crates_pick_their_tracks() {
        let db = LibraryDb::open_in_memory().unwrap();
        analysed(&db, "a.mp3", "Deep House");
        analysed(&db, "b.mp3", "Techno");
        failed(&db, "c.mp3");
        let house = SmartRules {
            match_all: true,
            rules: vec![Rule::Text { field: TextField::Genre, contains: "house".into() }],
        };
        let id = db.create_smart_crate("House", &house).unwrap();
        assert_eq!(db.crate_rules(id).unwrap(), Some(house));
        assert_eq!(db.crates().unwrap()[0].track_count, 1);
        assert_eq!(db.crate_tracks(id).unwrap()[0].path, "a.mp3");

        let techno = SmartRules {
            match_all: true,
            rules: vec![Rule::Text { field: TextField::Genre, contains: "techno".into() }],
        };
        db.set_crate_rules(id, &techno).unwrap();
        assert_eq!(db.crate_tracks(id).unwrap()[0].path, "b.mp3");
    }
}
