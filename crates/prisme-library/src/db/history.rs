//! History of the sets: each run of the app that plays tracks is a session, holding the
//! tracks played in order.

use super::tracks::record_from_row;
use super::{path_key, DbError, LibraryDb, TrackRecord};
use rusqlite::params;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistorySession {
    pub id: SessionId,
    pub started_at_ms: i64,
    pub play_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryPlay {
    pub path: String,
    pub deck: usize,
    /// When the track started playing.
    pub played_at_ms: i64,
    pub record: Option<TrackRecord>,
}

impl LibraryDb {
    pub fn start_history_session(&self, started_at_ms: i64) -> Result<SessionId, DbError> {
        let conn = self.conn();
        conn.prepare_cached("INSERT INTO history_sessions (started_at_ms) VALUES (?1)")?
            .execute([started_at_ms])?;
        Ok(SessionId(conn.last_insert_rowid()))
    }

    pub fn record_play(
        &self,
        session: SessionId,
        path: &Path,
        deck: usize,
        played_at_ms: i64,
    ) -> Result<(), DbError> {
        self.conn()
            .prepare_cached(
                "INSERT INTO history_plays (session_id, path, deck, played_at_ms) VALUES (?1, ?2, ?3, ?4)",
            )?
            .execute(params![session.0, path_key(path), deck as i64, played_at_ms])?;
        Ok(())
    }

    /// Every session, the latest first.
    pub fn history_sessions(&self) -> Result<Vec<HistorySession>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT history_sessions.id, history_sessions.started_at_ms, COUNT(history_plays.id)
             FROM history_sessions LEFT JOIN history_plays ON history_plays.session_id = history_sessions.id
             GROUP BY history_sessions.id ORDER BY history_sessions.started_at_ms DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(HistorySession {
                id: SessionId(r.get(0)?),
                started_at_ms: r.get(1)?,
                play_count: r.get::<_, i64>(2)? as usize,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The tracks of a session, in the order they were played.
    pub fn history_plays(&self, session: SessionId) -> Result<Vec<HistoryPlay>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT history_plays.path AS play_path, history_plays.deck, history_plays.played_at_ms, tracks.*
             FROM history_plays LEFT JOIN tracks ON tracks.path = history_plays.path
             WHERE history_plays.session_id = ?1 ORDER BY history_plays.played_at_ms, history_plays.id",
        )?;
        let rows = stmt.query_map([session.0], |r| {
            let analysed = r.get::<_, Option<String>>("path")?.is_some();
            Ok(HistoryPlay {
                path: r.get("play_path")?,
                deck: r.get::<_, i64>("deck")? as usize,
                played_at_ms: r.get("played_at_ms")?,
                record: if analysed { Some(record_from_row(r)?) } else { None },
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn delete_history_session(&self, session: SessionId) -> Result<(), DbError> {
        self.conn().prepare_cached("DELETE FROM history_sessions WHERE id = ?1")?.execute([session.0])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plays_are_kept_per_session_in_order() {
        let db = LibraryDb::open_in_memory().unwrap();
        let friday = db.start_history_session(1_000).unwrap();
        let saturday = db.start_history_session(90_000).unwrap();
        db.record_play(friday, Path::new("b.mp3"), 1, 2_000).unwrap();
        db.record_play(friday, Path::new("a.mp3"), 0, 1_500).unwrap();
        db.record_play(friday, Path::new("a.mp3"), 0, 3_000).unwrap();
        db.record_play(saturday, Path::new("c.mp3"), 0, 91_000).unwrap();

        let sessions = db.history_sessions().unwrap();
        assert_eq!(
            sessions.iter().map(|s| (s.id, s.play_count)).collect::<Vec<_>>(),
            [(saturday, 1), (friday, 3)]
        );

        let plays = db.history_plays(friday).unwrap();
        let order: Vec<_> = plays.iter().map(|p| (p.path.as_str(), p.deck, p.played_at_ms)).collect();
        assert_eq!(order, [("a.mp3", 0, 1_500), ("b.mp3", 1, 2_000), ("a.mp3", 0, 3_000)]);
        assert!(plays.iter().all(|p| p.record.is_none()));
    }

    #[test]
    fn deleting_a_session_deletes_its_plays() {
        let db = LibraryDb::open_in_memory().unwrap();
        let session = db.start_history_session(0).unwrap();
        db.record_play(session, Path::new("a.mp3"), 0, 1).unwrap();
        db.delete_history_session(session).unwrap();
        assert!(db.history_sessions().unwrap().is_empty());
        assert!(db.history_plays(session).unwrap().is_empty());
    }
}
