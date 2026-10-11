//! Corrections made by hand to a track's beat grid and key. They are kept apart from the
//! analysis results, so that analysing the file again does not undo them.

use super::{path_key, DbError, LibraryDb};
use prisme_analysis::{Key, KeyEstimate, Mode, Tempo, TrackAnalysis};
use rusqlite::{params, OptionalExtension, Row};
use std::ops::RangeInclusive;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridEdit {
    pub bpm: f64,
    /// The first beat at or after the start of the track.
    pub first_beat_s: f64,
}

impl GridEdit {
    pub const BPM_RANGE: RangeInclusive<f64> = 40.0..=300.0;

    fn period_s(&self) -> f64 {
        60.0 / self.bpm
    }

    /// The grid at another tempo, anchored on its first beat; `None` outside
    /// [`GridEdit::BPM_RANGE`].
    pub fn with_bpm(self, bpm: f64) -> Option<Self> {
        Self::BPM_RANGE.contains(&bpm).then_some(Self { bpm, ..self })
    }

    /// The grid moved by `seconds` (later when positive).
    pub fn shifted(self, seconds: f64) -> Self {
        Self { first_beat_s: (self.first_beat_s + seconds).rem_euclid(self.period_s()), ..self }
    }

    /// The grid moved so that a beat falls at `position_s`.
    pub fn with_beat_at(self, position_s: f64) -> Self {
        Self { first_beat_s: position_s.rem_euclid(self.period_s()), ..self }
    }
}

impl From<Tempo> for GridEdit {
    fn from(t: Tempo) -> Self {
        Self { bpm: t.bpm, first_beat_s: t.first_beat_s }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TrackEdits {
    pub grid: Option<GridEdit>,
    pub key: Option<Key>,
}

impl TrackEdits {
    pub fn is_empty(&self) -> bool {
        self.grid.is_none() && self.key.is_none()
    }

    /// The analysis with the corrections applied.
    pub fn apply(&self, detected: TrackAnalysis) -> TrackAnalysis {
        TrackAnalysis {
            tempo: self
                .grid
                .map(|g| Tempo { bpm: g.bpm, first_beat_s: g.first_beat_s, confidence: 1.0 })
                .or(detected.tempo),
            key: self
                .key
                .map(|key| KeyEstimate {
                    key,
                    confidence: 1.0,
                    tuning_cents: detected.key.map_or(0.0, |k| k.tuning_cents),
                })
                .or(detected.key),
        }
    }

    /// From the `edit_*` columns of `track_view` (or `track_edits` with the same names).
    pub(super) fn from_row(r: &Row<'_>) -> rusqlite::Result<Self> {
        let grid = match (r.get::<_, Option<f64>>("edit_bpm")?, r.get::<_, Option<f64>>("edit_first_beat_s")?)
        {
            (Some(bpm), Some(first_beat_s)) => Some(GridEdit { bpm, first_beat_s }),
            _ => None,
        };
        let key =
            match (r.get::<_, Option<u8>>("edit_key_tonic")?, r.get::<_, Option<bool>>("edit_key_minor")?) {
                (Some(tonic), Some(minor)) => {
                    Some(Key::new(tonic, if minor { Mode::Minor } else { Mode::Major }))
                }
                _ => None,
            };
        Ok(Self { grid, key })
    }
}

impl LibraryDb {
    pub fn edits(&self, path: &Path) -> Result<TrackEdits, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT bpm AS edit_bpm, first_beat_s AS edit_first_beat_s, key_tonic AS edit_key_tonic,
                 key_minor AS edit_key_minor FROM track_edits WHERE path = ?1",
        )?;
        Ok(stmt.query_row([path_key(path)], TrackEdits::from_row).optional()?.unwrap_or_default())
    }

    pub fn save_edits(&self, path: &Path, edits: &TrackEdits) -> Result<(), DbError> {
        let conn = self.conn();
        conn.prepare_cached("UPDATE tracks SET tags_written = 0 WHERE path = ?1")?
            .execute([path_key(path)])?;
        if edits.is_empty() {
            conn.prepare_cached("DELETE FROM track_edits WHERE path = ?1")?.execute([path_key(path)])?;
            return Ok(());
        }
        conn.prepare_cached(
            "INSERT OR REPLACE INTO track_edits (path, bpm, first_beat_s, key_tonic, key_minor)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?
        .execute(params![
            path_key(path),
            edits.grid.map(|g| g.bpm),
            edits.grid.map(|g| g.first_beat_s),
            edits.key.map(|k| k.tonic()),
            edits.key.map(|k| k.mode() == Mode::Minor),
        ])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{FileStamp, TrackRecord};
    use prisme_media::TrackTags;

    fn detected() -> TrackAnalysis {
        TrackAnalysis {
            tempo: Some(Tempo { bpm: 62.0, first_beat_s: 0.4, confidence: 0.5 }),
            key: Some(KeyEstimate { key: Key::new(9, Mode::Minor), confidence: 0.2, tuning_cents: -8.0 }),
        }
    }

    #[test]
    fn grid_corrections() {
        let grid = GridEdit { bpm: 120.0, first_beat_s: 0.1 };
        assert_eq!(grid.with_bpm(240.0), Some(GridEdit { bpm: 240.0, first_beat_s: 0.1 }));
        assert_eq!(grid.with_bpm(20.0), None);
        let close = |a: GridEdit, first_beat_s: f64| (a.first_beat_s - first_beat_s).abs() < 1e-9;
        assert!(close(grid.shifted(0.05), 0.15));
        assert!(close(grid.shifted(-0.15), 0.45), "the first beat stays within the first period");
        assert!(close(grid.shifted(0.5), 0.1), "a whole beat later is the same grid");
        assert!(close(grid.with_beat_at(10.3), 0.3));
    }

    #[test]
    fn edits_override_the_detected_values() {
        let edits = TrackEdits {
            grid: Some(GridEdit { bpm: 124.0, first_beat_s: 0.15 }),
            key: Some(Key::new(0, Mode::Major)),
        };
        let applied = edits.apply(detected());
        assert_eq!(applied.tempo, Some(Tempo { bpm: 124.0, first_beat_s: 0.15, confidence: 1.0 }));
        let key = applied.key.unwrap();
        assert_eq!((key.key, key.tuning_cents), (Key::new(0, Mode::Major), -8.0));
        assert_eq!(TrackEdits::default().apply(detected()), detected());
    }

    #[test]
    fn edits_survive_a_new_analysis() {
        let db = LibraryDb::open_in_memory().unwrap();
        let path = Path::new("a.mp3");
        let stamp = FileStamp { size: 1, modified_ms: 1 };
        let edits = TrackEdits { grid: Some(GridEdit { bpm: 124.0, first_beat_s: 0.15 }), key: None };
        db.save(&TrackRecord::analyzed(path, stamp, 100.0, detected(), TrackTags::default())).unwrap();
        db.save_edits(path, &edits).unwrap();
        db.save(&TrackRecord::analyzed(path, stamp, 100.0, detected(), TrackTags::default())).unwrap();

        let record = db.get(path).unwrap().unwrap();
        assert_eq!(record.edits, edits);
        assert_eq!(record.effective_analysis().tempo.unwrap().bpm, 124.0);
        assert_eq!(db.edits(path).unwrap(), edits);

        db.save_edits(path, &TrackEdits::default()).unwrap();
        assert_eq!(db.get(path).unwrap().unwrap().edits, TrackEdits::default());
    }
}
