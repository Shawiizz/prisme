//! Track database (SQLite).
//!
//! One row per file, keyed by path. A row is valid for one version of the file (its
//! size and modification time, [`FileStamp`]) and one version of the analysis
//! algorithms ([`ANALYSIS_VERSION`]); otherwise the file is analysed again.

use prisme_analysis::{Key, KeyEstimate, Mode, Tempo, TrackAnalysis, ANALYSIS_VERSION};
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::collections::HashMap;
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
const MIGRATIONS: &[&str] = &["CREATE TABLE tracks (
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
    ) WITHOUT ROWID;"];

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

/// What the library knows about a file.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackRecord {
    pub path: String,
    pub stamp: FileStamp,
    pub analysis_version: u32,
    pub duration_s: Option<f64>,
    pub analysis: TrackAnalysis,
    /// Why the file could not be analysed.
    pub error: Option<String>,
}

impl TrackRecord {
    pub fn analyzed(path: &Path, stamp: FileStamp, duration_s: f64, analysis: TrackAnalysis) -> Self {
        Self {
            path: path_key(path),
            stamp,
            analysis_version: ANALYSIS_VERSION,
            duration_s: Some(duration_s),
            analysis,
            error: None,
        }
    }

    pub fn failed(path: &Path, stamp: FileStamp, error: String) -> Self {
        Self {
            path: path_key(path),
            stamp,
            analysis_version: ANALYSIS_VERSION,
            duration_s: None,
            analysis: TrackAnalysis { tempo: None, key: None },
            error: Some(error),
        }
    }

    /// Whether the record describes this version of the file with the current
    /// algorithms.
    pub fn is_current(&self, stamp: FileStamp) -> bool {
        self.stamp == stamp && self.analysis_version == ANALYSIS_VERSION
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

    pub fn get(&self, path: &Path) -> Result<Option<TrackRecord>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached("SELECT * FROM tracks WHERE path = ?1")?;
        Ok(stmt.query_row([path_key(path)], record_from_row).optional()?)
    }

    /// Records of the given paths that exist in the database.
    pub fn get_many<'a>(
        &self,
        paths: impl IntoIterator<Item = &'a Path>,
    ) -> Result<HashMap<String, TrackRecord>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached("SELECT * FROM tracks WHERE path = ?1")?;
        let mut found = HashMap::new();
        for path in paths {
            if let Some(r) = stmt.query_row([path_key(path)], record_from_row).optional()? {
                found.insert(r.path.clone(), r);
            }
        }
        Ok(found)
    }

    /// File stamp and analysis version of every stored track (to find stale ones).
    pub fn versions(&self) -> Result<HashMap<String, (FileStamp, u32)>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT path, size, modified_ms, analysis_version FROM tracks")?;
        let rows = stmt.query_map([], |r| {
            let stamp = FileStamp { size: r.get::<_, i64>(1)? as u64, modified_ms: r.get(2)? };
            Ok((r.get::<_, String>(0)?, (stamp, r.get(3)?)))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn save(&self, record: &TrackRecord) -> Result<(), DbError> {
        let tempo = record.analysis.tempo;
        let key = record.analysis.key;
        let now_ms = std::time::SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
        self.conn().prepare_cached(
            "INSERT OR REPLACE INTO tracks (path, size, modified_ms, analysis_version, duration_s, bpm, first_beat_s,
                 tempo_confidence, key_tonic, key_minor, key_confidence, tuning_cents, error, analyzed_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        )?
        .execute(params![
            record.path,
            record.stamp.size as i64,
            record.stamp.modified_ms,
            record.analysis_version,
            record.duration_s,
            tempo.map(|t| t.bpm),
            tempo.map(|t| t.first_beat_s),
            tempo.map(|t| t.confidence),
            key.map(|k| k.key.tonic()),
            key.map(|k| k.key.mode() == Mode::Minor),
            key.map(|k| k.confidence),
            key.map(|k| k.tuning_cents),
            record.error,
            now_ms,
        ])?;
        Ok(())
    }

    pub fn track_count(&self) -> Result<usize, DbError> {
        Ok(self.conn().query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get::<_, i64>(0))? as usize)
    }
}

fn record_from_row(r: &Row<'_>) -> rusqlite::Result<TrackRecord> {
    let tempo = match (r.get::<_, Option<f64>>("bpm")?, r.get::<_, Option<f64>>("first_beat_s")?) {
        (Some(bpm), Some(first_beat_s)) => Some(Tempo {
            bpm,
            first_beat_s,
            confidence: r.get::<_, Option<f32>>("tempo_confidence")?.unwrap_or(0.0),
        }),
        _ => None,
    };
    let key = match (r.get::<_, Option<u8>>("key_tonic")?, r.get::<_, Option<bool>>("key_minor")?) {
        (Some(tonic), Some(minor)) => Some(KeyEstimate {
            key: Key::new(tonic, if minor { Mode::Minor } else { Mode::Major }),
            confidence: r.get::<_, Option<f32>>("key_confidence")?.unwrap_or(0.0),
            tuning_cents: r.get::<_, Option<f32>>("tuning_cents")?.unwrap_or(0.0),
        }),
        _ => None,
    };
    Ok(TrackRecord {
        path: r.get("path")?,
        stamp: FileStamp { size: r.get::<_, i64>("size")? as u64, modified_ms: r.get("modified_ms")? },
        analysis_version: r.get("analysis_version")?,
        duration_s: r.get("duration_s")?,
        analysis: TrackAnalysis { tempo, key },
        error: r.get("error")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(path: &str) -> TrackRecord {
        TrackRecord::analyzed(
            Path::new(path),
            FileStamp { size: 1234, modified_ms: 99 },
            301.5,
            TrackAnalysis {
                tempo: Some(Tempo { bpm: 124.0, first_beat_s: 0.212, confidence: 0.9 }),
                key: Some(KeyEstimate { key: Key::new(9, Mode::Minor), confidence: 0.3, tuning_cents: -4.0 }),
            },
        )
    }

    #[test]
    fn records_round_trip() {
        let db = LibraryDb::open_in_memory().unwrap();
        let record = sample("C:/Music/a.mp3");
        db.save(&record).unwrap();
        assert_eq!(db.get(Path::new("C:/Music/a.mp3")).unwrap(), Some(record));
        assert_eq!(db.get(Path::new("C:/Music/missing.mp3")).unwrap(), None);

        let failed = TrackRecord::failed(Path::new("C:/Music/b.mp3"), FileStamp { size: 1, modified_ms: 2 }, "corrupt".into());
        db.save(&failed).unwrap();
        assert_eq!(db.get(Path::new("C:/Music/b.mp3")).unwrap(), Some(failed));
        assert_eq!(db.track_count().unwrap(), 2);
    }

    #[test]
    fn saving_again_replaces_the_record() {
        let db = LibraryDb::open_in_memory().unwrap();
        db.save(&sample("x.mp3")).unwrap();
        let mut newer = sample("x.mp3");
        newer.stamp.size = 999;
        newer.analysis.key = None;
        db.save(&newer).unwrap();
        assert_eq!(db.track_count().unwrap(), 1);
        assert_eq!(db.get(Path::new("x.mp3")).unwrap(), Some(newer));
    }

    #[test]
    fn a_changed_file_or_algorithm_makes_the_record_stale() {
        let r = sample("x.mp3");
        assert!(r.is_current(FileStamp { size: 1234, modified_ms: 99 }));
        assert!(!r.is_current(FileStamp { size: 1234, modified_ms: 100 }));
        assert!(!TrackRecord { analysis_version: ANALYSIS_VERSION + 1, ..r }.is_current(FileStamp { size: 1234, modified_ms: 99 }));
    }

    #[test]
    fn reopening_keeps_the_data_and_the_schema() {
        let dir = std::env::temp_dir().join(format!("prisme-db-test-{}", std::process::id()));
        let file = dir.join("library.sqlite");
        {
            let db = LibraryDb::open(&file).unwrap();
            db.save(&sample("y.mp3")).unwrap();
        }
        let db = LibraryDb::open(&file).unwrap();
        assert_eq!(db.track_count().unwrap(), 1);
        drop(db);
        std::fs::remove_dir_all(dir).ok();
    }
}
