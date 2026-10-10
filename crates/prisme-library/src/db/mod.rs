//! Library database (SQLite): tracks and their analysis and tags ([`tracks`]), cues,
//! cover thumbnails, full-text search, crates.

mod covers;
mod crates;
mod cues;
mod tracks;

pub use crates::{Crate, CrateEntry, CrateId};
pub use tracks::{RecordVersion, TrackRecord, TAGS_VERSION};

use rusqlite::Connection;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::UNIX_EPOCH;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("library database: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("library database: {0}")]
    Io(#[from] std::io::Error),
}

/// Schema changes, applied in order; the database records how many it has seen
/// (`PRAGMA user_version`).
const MIGRATIONS: &[&str] = &[
    "CREATE TABLE tracks (
        path TEXT PRIMARY KEY NOT NULL,
        size INTEGER NOT NULL,
        modified_ms INTEGER NOT NULL,
        analysis_version INTEGER NOT NULL,
        duration_s REAL,
        bpm REAL,
        first_beat_s REAL,
        tempo_confidence REAL,
        key_tonic INTEGER,
        key_minor INTEGER,
        key_confidence REAL,
        tuning_cents REAL,
        error TEXT,
        analyzed_at_ms INTEGER NOT NULL
    ) WITHOUT ROWID;",
    // cues outlive analysis results: they are not in the tracks table
    "CREATE TABLE cues (
        path TEXT NOT NULL,
        slot INTEGER NOT NULL,
        position_s REAL NOT NULL,
        loop_length_s REAL,
        color INTEGER NOT NULL,
        label TEXT NOT NULL,
        PRIMARY KEY (path, slot)
    ) WITHOUT ROWID;",
    "ALTER TABLE tracks ADD COLUMN title TEXT;
     ALTER TABLE tracks ADD COLUMN artist TEXT;
     ALTER TABLE tracks ADD COLUMN album TEXT;
     ALTER TABLE tracks ADD COLUMN genre TEXT;
     ALTER TABLE tracks ADD COLUMN year INTEGER;
     ALTER TABLE tracks ADD COLUMN comment TEXT;
     ALTER TABLE tracks ADD COLUMN tags_version INTEGER NOT NULL DEFAULT 0;
     CREATE TABLE covers (path TEXT PRIMARY KEY NOT NULL, jpeg BLOB NOT NULL) WITHOUT ROWID;
     CREATE VIRTUAL TABLE track_search USING fts5(
         path UNINDEXED, words, tokenize = 'unicode61 remove_diacritics 2'
     );",
    "CREATE TABLE crates (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
     CREATE TABLE crate_tracks (
         crate_id INTEGER NOT NULL REFERENCES crates (id) ON DELETE CASCADE,
         path TEXT NOT NULL,
         added_at_ms INTEGER NOT NULL,
         PRIMARY KEY (crate_id, path)
     );",
];

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// The key a path is stored under.
pub fn path_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Identifies one version of a file: when it changes, stored results are stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileStamp {
    pub size: u64,
    pub modified_ms: i64,
}

impl FileStamp {
    pub fn of(path: &Path) -> std::io::Result<Self> {
        Ok(Self::from_metadata(&std::fs::metadata(path)?))
    }

    pub fn from_metadata(meta: &std::fs::Metadata) -> Self {
        let modified_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis() as i64);
        Self { size: meta.len(), modified_ms }
    }
}

pub struct LibraryDb {
    conn: Mutex<Connection>,
}

impl LibraryDb {
    /// Opens (or creates) the database file.
    pub fn open(path: &Path) -> Result<Self, DbError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self, DbError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, DbError> {
        // WAL: readers (the UI) are not blocked by the analysis writing
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        let applied: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(applied as usize) {
            conn.execute_batch(sql)?;
            conn.pragma_update(None, "user_version", i as i64 + 1)?;
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reopening_keeps_the_data_and_the_schema() {
        let dir = std::env::temp_dir().join(format!("prisme-db-test-{}", std::process::id()));
        let file = dir.join("library.sqlite");
        {
            let db = LibraryDb::open(&file).unwrap();
            db.save(&TrackRecord::failed(
                Path::new("y.mp3"),
                FileStamp { size: 1, modified_ms: 2 },
                "x".into(),
            ))
            .unwrap();
        }
        let db = LibraryDb::open(&file).unwrap();
        assert_eq!(db.track_count().unwrap(), 1);
        drop(db);
        std::fs::remove_dir_all(dir).ok();
    }
}
