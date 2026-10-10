//! One row per file, keyed by path. A row is valid for one version of the file (its
//! size and modification time, [`FileStamp`]), of the analysis algorithms
//! ([`ANALYSIS_VERSION`]) and of the tag reading ([`TAGS_VERSION`]); otherwise the file
//! is read again.

use super::edits::TrackEdits;
use super::{path_key, DbError, FileStamp, LibraryDb};
use prisme_analysis::{Key, KeyEstimate, Mode, Tempo, TrackAnalysis, ANALYSIS_VERSION};
use prisme_media::TrackTags;
use rusqlite::{params, OptionalExtension, Row};
use std::collections::HashMap;
use std::path::Path;

/// Version of what is read from the tags: bump it to read every file's tags again.
pub const TAGS_VERSION: u32 = 1;

/// A track of a list (crate, prepare list), with what the library knows of it (`None`
/// if it was never analysed).
#[derive(Debug, Clone, PartialEq)]
pub struct TrackEntry {
    pub path: String,
    pub record: Option<TrackRecord>,
}

/// What the library knows about a file.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackRecord {
    pub path: String,
    pub version: RecordVersion,
    pub duration_s: Option<f64>,
    /// As detected; see [`TrackRecord::effective_analysis`].
    pub analysis: TrackAnalysis,
    pub edits: TrackEdits,
    pub tags: TrackTags,
    /// Why the file could not be analysed.
    pub error: Option<String>,
}

/// What a record was made from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordVersion {
    pub stamp: FileStamp,
    pub analysis_version: u32,
    pub tags_version: u32,
}

impl RecordVersion {
    pub fn current(stamp: FileStamp) -> Self {
        Self { stamp, analysis_version: ANALYSIS_VERSION, tags_version: TAGS_VERSION }
    }

    /// Whether the record describes this version of the file with the current code.
    pub fn is_current(&self, stamp: FileStamp) -> bool {
        *self == Self::current(stamp)
    }

    /// Whether the analysis is still valid for this version of the file (the tags may
    /// not be).
    pub fn analysis_is_current(&self, stamp: FileStamp) -> bool {
        self.stamp == stamp && self.analysis_version == ANALYSIS_VERSION
    }
}

impl TrackRecord {
    pub fn analyzed(
        path: &Path,
        stamp: FileStamp,
        duration_s: f64,
        analysis: TrackAnalysis,
        tags: TrackTags,
    ) -> Self {
        Self {
            path: path_key(path),
            version: RecordVersion::current(stamp),
            duration_s: Some(duration_s),
            analysis,
            edits: TrackEdits::default(),
            tags,
            error: None,
        }
    }

    pub fn failed(path: &Path, stamp: FileStamp, error: String) -> Self {
        Self {
            path: path_key(path),
            version: RecordVersion::current(stamp),
            duration_s: None,
            analysis: TrackAnalysis { tempo: None, key: None },
            edits: TrackEdits::default(),
            tags: TrackTags::default(),
            error: Some(error),
        }
    }

    pub fn is_current(&self, stamp: FileStamp) -> bool {
        self.version.is_current(stamp)
    }

    /// The analysis with the corrections made by hand.
    pub fn effective_analysis(&self) -> TrackAnalysis {
        self.edits.apply(self.analysis)
    }

    /// The same record with tags read by the current code.
    pub fn with_tags(self, tags: TrackTags) -> Self {
        Self { tags, version: RecordVersion { tags_version: TAGS_VERSION, ..self.version }, ..self }
    }

    /// What full-text search looks into: the tags and the file name.
    fn search_words(&self) -> String {
        let stem = Path::new(&self.path).file_stem().map(|s| s.to_string_lossy().into_owned());
        let t = &self.tags;
        [&t.title, &t.artist, &t.album, &t.genre, &stem]
            .into_iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl LibraryDb {
    pub fn get(&self, path: &Path) -> Result<Option<TrackRecord>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached("SELECT * FROM track_view WHERE path = ?1")?;
        Ok(stmt.query_row([path_key(path)], record_from_row).optional()?)
    }

    /// What every stored record was made from (to find the stale ones).
    pub fn versions(&self) -> Result<HashMap<String, RecordVersion>, DbError> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare("SELECT path, size, modified_ms, analysis_version, tags_version FROM tracks")?;
        let rows = stmt.query_map([], |r| {
            let stamp = FileStamp { size: r.get::<_, i64>(1)? as u64, modified_ms: r.get(2)? };
            Ok((
                r.get::<_, String>(0)?,
                RecordVersion { stamp, analysis_version: r.get(3)?, tags_version: r.get(4)? },
            ))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Stores the record and indexes it for search.
    pub fn save(&self, record: &TrackRecord) -> Result<(), DbError> {
        let tempo = record.analysis.tempo;
        let key = record.analysis.key;
        let tags = &record.tags;
        let now_ms = super::now_ms();
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        tx.prepare_cached(
            "INSERT OR REPLACE INTO tracks (path, size, modified_ms, analysis_version, duration_s, bpm,
                 first_beat_s, tempo_confidence, key_tonic, key_minor, key_confidence, tuning_cents, error,
                 analyzed_at_ms, title, artist, album, genre, year, comment, tags_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19,
                 ?20, ?21)",
        )?
        .execute(params![
            record.path,
            record.version.stamp.size as i64,
            record.version.stamp.modified_ms,
            record.version.analysis_version,
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
            tags.title,
            tags.artist,
            tags.album,
            tags.genre,
            tags.year,
            tags.comment,
            record.version.tags_version,
        ])?;
        tx.prepare_cached("DELETE FROM track_search WHERE path = ?1")?.execute([&record.path])?;
        tx.prepare_cached("INSERT INTO track_search (path, words) VALUES (?1, ?2)")?
            .execute(params![record.path, record.search_words()])?;
        tx.commit()?;
        Ok(())
    }

    /// Tracks whose tags or file name contain every word of `query` (word starts
    /// match: "dee mau" finds deadmau5), best matches first.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<TrackRecord>, DbError> {
        let Some(fts) = fts_query(query) else { return Ok(Vec::new()) };
        let conn = self.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT track_view.* FROM track_search JOIN track_view ON track_view.path = track_search.path
             WHERE track_search MATCH ?1 ORDER BY rank LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![fts, limit as i64], record_from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn track_count(&self) -> Result<usize, DbError> {
        Ok(self.conn().query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get::<_, i64>(0))? as usize)
    }
}

/// `deadmau5 alone` -> `"deadmau5"* "alone"*`; `None` when there is nothing to look for.
fn fts_query(query: &str) -> Option<String> {
    let terms: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

/// A row of `SELECT <list>.path AS entry_path, track_view.* ... LEFT JOIN track_view`.
pub(super) fn entry_from_row(r: &Row<'_>) -> rusqlite::Result<TrackEntry> {
    let analysed = r.get::<_, Option<String>>("path")?.is_some();
    Ok(TrackEntry {
        path: r.get("entry_path")?,
        record: if analysed { Some(record_from_row(r)?) } else { None },
    })
}

pub(super) fn record_from_row(r: &Row<'_>) -> rusqlite::Result<TrackRecord> {
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
    let stamp = FileStamp { size: r.get::<_, i64>("size")? as u64, modified_ms: r.get("modified_ms")? };
    Ok(TrackRecord {
        path: r.get("path")?,
        version: RecordVersion {
            stamp,
            analysis_version: r.get("analysis_version")?,
            tags_version: r.get("tags_version")?,
        },
        duration_s: r.get("duration_s")?,
        analysis: TrackAnalysis { tempo, key },
        edits: TrackEdits::from_row(r)?,
        tags: TrackTags {
            title: r.get("title")?,
            artist: r.get("artist")?,
            album: r.get("album")?,
            genre: r.get("genre")?,
            year: r.get("year")?,
            comment: r.get("comment")?,
        },
        error: r.get("error")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAMP: FileStamp = FileStamp { size: 1234, modified_ms: 99 };

    fn sample(path: &str, artist: &str, title: &str) -> TrackRecord {
        TrackRecord::analyzed(
            Path::new(path),
            STAMP,
            301.5,
            TrackAnalysis {
                tempo: Some(Tempo { bpm: 124.0, first_beat_s: 0.212, confidence: 0.9 }),
                key: Some(KeyEstimate { key: Key::new(9, Mode::Minor), confidence: 0.3, tuning_cents: -4.0 }),
            },
            TrackTags {
                title: Some(title.into()),
                artist: Some(artist.into()),
                album: Some("Album".into()),
                genre: Some("House".into()),
                year: Some(2024),
                comment: None,
            },
        )
    }

    #[test]
    fn records_round_trip() {
        let db = LibraryDb::open_in_memory().unwrap();
        let record = sample("C:/Music/a.mp3", "deadmau5", "Alone with You");
        db.save(&record).unwrap();
        assert_eq!(db.get(Path::new("C:/Music/a.mp3")).unwrap(), Some(record));
        assert_eq!(db.get(Path::new("C:/Music/missing.mp3")).unwrap(), None);

        let failed = TrackRecord::failed(Path::new("C:/Music/b.mp3"), STAMP, "corrupt".into());
        db.save(&failed).unwrap();
        assert_eq!(db.get(Path::new("C:/Music/b.mp3")).unwrap(), Some(failed));
        assert_eq!(db.track_count().unwrap(), 2);
    }

    #[test]
    fn saving_again_replaces_the_record_and_its_search_entry() {
        let db = LibraryDb::open_in_memory().unwrap();
        db.save(&sample("x.mp3", "Old Artist", "Old Title")).unwrap();
        let newer = sample("x.mp3", "New Artist", "New Title");
        db.save(&newer).unwrap();
        assert_eq!(db.track_count().unwrap(), 1);
        assert_eq!(db.get(Path::new("x.mp3")).unwrap(), Some(newer));
        assert!(db.search("old", 10).unwrap().is_empty());
        assert_eq!(db.search("new", 10).unwrap().len(), 1);
    }

    #[test]
    fn a_changed_file_or_code_makes_the_record_stale() {
        let r = sample("x.mp3", "a", "b");
        assert!(r.is_current(STAMP));
        assert!(!r.is_current(FileStamp { size: 1234, modified_ms: 100 }));
        let older_tags = RecordVersion { tags_version: TAGS_VERSION - 1, ..r.version };
        assert!(!older_tags.is_current(STAMP));
        assert!(older_tags.analysis_is_current(STAMP));
    }

    #[test]
    fn retagging_keeps_the_analysis() {
        let mut stale = sample("x.mp3", "a", "b");
        stale.version.tags_version = 0;
        let tags = TrackTags { title: Some("New".into()), ..TrackTags::default() };
        let retagged = stale.clone().with_tags(tags.clone());
        assert!(retagged.is_current(STAMP));
        assert_eq!(retagged.tags, tags);
        assert_eq!(retagged.analysis, stale.analysis);
    }

    #[test]
    fn search_finds_word_starts_in_tags_and_file_names() {
        let db = LibraryDb::open_in_memory().unwrap();
        db.save(&sample("C:/Music/House/alone.mp3", "deadmau5", "Alone with You")).unwrap();
        db.save(&sample("C:/Music/House/b.mp3", "Claptone", "No Eyes")).unwrap();
        db.save(&TrackRecord::failed(Path::new("C:/Music/Schranz/Café Noir - Écho.mp3"), STAMP, "x".into()))
            .unwrap();
        let found = |q: &str| db.search(q, 10).unwrap().into_iter().map(|r| r.path).collect::<Vec<_>>();
        assert_eq!(found("dead alo"), ["C:/Music/House/alone.mp3"]);
        assert_eq!(found("eyes CLAPTONE"), ["C:/Music/House/b.mp3"]);
        assert_eq!(
            found("cafe echo"),
            ["C:/Music/Schranz/Café Noir - Écho.mp3"],
            "file name, accents ignored"
        );
        assert!(found("house").len() == 2, "genre");
        assert!(found("  ").is_empty());
        assert!(found("\"; DROP TABLE tracks; --").is_empty());
    }
}
