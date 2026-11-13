//! What tells a file again once it is elsewhere: the identity of its audio
//! ([`prisme_media::audio_id`]), kept by path even after the track is forgotten, as its
//! cues and corrections are; and its size and time.

use super::{path_key, DbError, FileStamp, LibraryDb};
use rusqlite::{params, OptionalExtension};
use std::collections::HashMap;
use std::path::Path;

impl LibraryDb {
    /// The file of `path` sounds like `id`.
    pub fn set_audio_id(&self, path: &Path, id: u64) -> Result<(), DbError> {
        self.conn()
            .prepare_cached("INSERT OR REPLACE INTO audio_ids (path, audio_id) VALUES (?1, ?2)")?
            .execute(params![path_key(path), id as i64])?;
        Ok(())
    }

    pub fn audio_id(&self, path: &Path) -> Result<Option<u64>, DbError> {
        let id: Option<i64> = self
            .conn()
            .prepare_cached("SELECT audio_id FROM audio_ids WHERE path = ?1")?
            .query_row([path_key(path)], |r| r.get(0))
            .optional()?;
        Ok(id.map(|id| id as u64))
    }

    /// Every path whose audio is known, with its identity.
    pub fn audio_ids(&self) -> Result<HashMap<String, u64>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT path, audio_id FROM audio_ids")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The paths whose file sounded like `id`.
    pub fn paths_sounding_like(&self, id: u64) -> Result<Vec<String>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached("SELECT path FROM audio_ids WHERE audio_id = ?1")?;
        let rows = stmt.query_map([id as i64], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The tracks stored from a file of this size and time.
    pub fn paths_stamped(&self, stamp: FileStamp) -> Result<Vec<String>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached("SELECT path FROM tracks WHERE size = ?1 AND modified_ms = ?2")?;
        let rows = stmt.query_map(params![stamp.size as i64, stamp.modified_ms], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The stored tracks whose audio has no identity yet.
    pub fn paths_without_audio_id(&self) -> Result<Vec<String>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT path FROM tracks WHERE error IS NULL AND path NOT IN (SELECT path FROM audio_ids)",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Whether a track is stored at `path`.
    pub fn has_track(&self, path: &Path) -> Result<bool, DbError> {
        Ok(self.conn().prepare_cached("SELECT 1 FROM tracks WHERE path = ?1")?.exists([path_key(path)])?)
    }
}
