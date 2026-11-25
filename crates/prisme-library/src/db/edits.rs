//! Corrections made by hand to a track's beat grid and key. They are kept apart from the
//! analysis results, so that analysing the file again does not undo them.

use super::{path_key, DbError, LibraryDb};
use prisme_analysis::{octave_ratio, Key, KeyEstimate, Mode, Tempo, TrackAnalysis};
use rusqlite::{params, OptionalExtension, Row};
use std::ops::RangeInclusive;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridEdit {
    pub bpm: f64,
    /// The first beat of bar 1 (see [`Tempo::first_beat_s`]).
    pub first_beat_s: f64,
    /// The tempo and bars from another program, the beats where the analysis puts them: that
    /// program may place its beats a little apart from the kicks (see [`GridEdit::on_beats_of`]).
    pub beats_from_analysis: bool,
}

/// Imported tempos closer than this to the analysis's (an octave apart or not) are the same.
const SAME_TEMPO: f64 = 0.001;
/// An imported grid's beats this close to the analysis's are the same beats, placed apart.
const SAME_BEATS_S: f64 = 0.06;

impl GridEdit {
    pub const BPM_RANGE: RangeInclusive<f64> = 40.0..=300.0;

    pub fn new(bpm: f64, first_beat_s: f64) -> Self {
        Self { bpm, first_beat_s, beats_from_analysis: false }
    }

    /// Another program's grid: see [`GridEdit::beats_from_analysis`].
    pub fn imported(bpm: f64, first_beat_s: f64) -> Self {
        Self { beats_from_analysis: true, ..Self::new(bpm, first_beat_s) }
    }

    fn period_s(&self) -> f64 {
        60.0 / self.bpm
    }

    /// The grid at another tempo, anchored on its first beat; `None` outside
    /// [`GridEdit::BPM_RANGE`].
    pub fn with_bpm(self, bpm: f64) -> Option<Self> {
        Self::BPM_RANGE.contains(&bpm).then_some(Self { bpm, ..self })
    }

    /// The grid moved by `seconds` (later when positive), its bars with it.
    pub fn shifted(self, seconds: f64) -> Self {
        Self { first_beat_s: self.first_beat_s + seconds, ..self }
    }

    /// The grid moved the least so that a beat falls at `position_s`, its bars with it.
    pub fn with_beat_at(self, position_s: f64) -> Self {
        let period = self.period_s();
        let late = (position_s - self.first_beat_s).rem_euclid(period);
        self.shifted(if late > period / 2.0 { late - period } else { late })
    }

    /// The grid moved the least so that a beat falls at `position_s`, a bar starting on it: bar
    /// 1 the bar's nearest the one before.
    pub fn with_bar_at(self, position_s: f64) -> Self {
        let bar = f64::from(prisme_analysis::BEATS_PER_BAR) * self.period_s();
        let bars = ((self.with_beat_at(position_s).first_beat_s - position_s) / bar).round();
        Self { first_beat_s: position_s + bars * bar, ..self }
    }

    /// An imported grid moved the least onto the beats of `analysed` in the middle of the track
    /// (`duration_s` long), if it is the same tempo and its beats fall near them: its tempo
    /// and bars stay. Any other grid as it is.
    fn on_beats_of(self, analysed: Option<Tempo>, duration_s: Option<f64>) -> Self {
        let Some(analysed) = analysed.filter(|_| self.beats_from_analysis) else { return self };
        let octave = octave_ratio(self.bpm, analysed.bpm);
        if (self.bpm / octave / analysed.bpm - 1.0).abs() > SAME_TEMPO {
            return self;
        }
        let nearest = |first_beat_s: f64, period: f64, at: f64| {
            first_beat_s + ((at - first_beat_s) / period).round() * period
        };
        let middle = duration_s.map_or(self.first_beat_s, |d| d / 2.0);
        let own = nearest(self.first_beat_s, self.period_s(), middle);
        let theirs = nearest(analysed.first_beat_s, analysed.beat_period_s(), middle);
        // the beats of the slower grid are beats of the faster one too
        let period = self.period_s().min(analysed.beat_period_s());
        let gap = theirs - own - ((theirs - own) / period).round() * period;
        if gap.abs() > SAME_BEATS_S {
            return self;
        }
        self.shifted(gap)
    }
}

impl From<Tempo> for GridEdit {
    fn from(t: Tempo) -> Self {
        Self::new(t.bpm, t.first_beat_s)
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

    /// The analysis of a track `duration_s` long with the corrections applied.
    pub fn apply(&self, detected: TrackAnalysis, duration_s: Option<f64>) -> TrackAnalysis {
        TrackAnalysis {
            tempo: self
                .grid
                .map(|g| g.on_beats_of(detected.tempo, duration_s))
                .map(|g| Tempo { bpm: g.bpm, first_beat_s: g.first_beat_s, confidence: 1.0, bars_sure: true })
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
            (Some(bpm), Some(first_beat_s)) => {
                Some(GridEdit { bpm, first_beat_s, beats_from_analysis: r.get("edit_beats_from_analysis")? })
            }
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
            "SELECT bpm AS edit_bpm, first_beat_s AS edit_first_beat_s,
                 beats_from_analysis AS edit_beats_from_analysis, key_tonic AS edit_key_tonic,
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
            "INSERT OR REPLACE INTO track_edits (path, bpm, first_beat_s, beats_from_analysis,
                 key_tonic, key_minor)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?
        .execute(params![
            path_key(path),
            edits.grid.map(|g| g.bpm),
            edits.grid.map(|g| g.first_beat_s),
            edits.grid.is_some_and(|g| g.beats_from_analysis),
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
            tempo: Some(Tempo { bpm: 62.0, first_beat_s: 0.4, confidence: 0.5, bars_sure: true }),
            key: Some(KeyEstimate { key: Key::new(9, Mode::Minor), confidence: 0.2, tuning_cents: -8.0 }),
        }
    }

    #[test]
    fn grid_corrections() {
        let grid = GridEdit::new(120.0, 0.1);
        assert_eq!(grid.with_bpm(240.0), Some(GridEdit::new(240.0, 0.1)));
        assert_eq!(grid.with_bpm(20.0), None);
        let close = |a: GridEdit, first_beat_s: f64| (a.first_beat_s - first_beat_s).abs() < 1e-9;
        assert!(close(grid.shifted(0.05), 0.15));
        assert!(close(grid.shifted(-0.15), -0.05), "the bars move with the beats");
        assert!(close(grid.shifted(0.5), 0.6), "a beat later, the bars start a beat later");
        assert!(close(grid.with_beat_at(10.3), 0.3));
        assert!(close(grid.with_beat_at(10.05), 0.05), "moved the least: earlier");
        assert!(close(grid.with_bar_at(10.3), 0.3), "bar 1 stays the nearest, ten seconds being five bars");
        assert!(close(grid.with_bar_at(11.3), -0.7), "half way between two bars 1, the earlier");
    }

    #[test]
    fn edits_override_the_detected_values() {
        let edits =
            TrackEdits { grid: Some(GridEdit::new(124.0, 0.15)), key: Some(Key::new(0, Mode::Major)) };
        let applied = edits.apply(detected(), Some(100.0));
        assert_eq!(
            applied.tempo,
            Some(Tempo { bpm: 124.0, first_beat_s: 0.15, confidence: 1.0, bars_sure: true })
        );
        let key = applied.key.unwrap();
        assert_eq!((key.key, key.tuning_cents), (Key::new(0, Mode::Major), -8.0));
        assert_eq!(TrackEdits::default().apply(detected(), Some(100.0)), detected());
    }

    #[test]
    fn an_imported_grid_keeps_its_tempo_and_bars_on_the_analysed_beats() {
        let applied = |grid: GridEdit, bpm: f64, first_beat_s: f64| {
            let detected = TrackAnalysis {
                tempo: Some(Tempo { bpm, first_beat_s, confidence: 0.5, bars_sure: true }),
                key: None,
            };
            let tempo =
                TrackEdits { grid: Some(grid), key: None }.apply(detected, Some(200.0)).tempo.unwrap();
            (tempo.bpm, (tempo.first_beat_s * 100.0).round() / 100.0)
        };
        let imported = GridEdit::imported(124.002, 2.031);
        // the analysis's beats on 2 s, four beats before it
        let on_two = 2.0 - 4.0 * 60.0 / 124.0;
        assert_eq!(applied(imported, 124.0, on_two), (124.002, 2.0), "on the beats, bar 1 kept");
        assert_eq!(applied(imported, 62.0, on_two), (124.002, 2.0), "an octave apart");
        assert_eq!(applied(imported, 248.0, on_two), (124.002, 2.0), "an octave apart, the other way");
        let off_beats = on_two + 30.0 / 124.0;
        assert_eq!(applied(imported, 124.0, off_beats), (124.002, 2.03), "the analysis on other beats");
        assert_eq!(applied(imported, 126.0, on_two), (124.002, 2.03), "another tempo");
        assert_eq!(applied(GridEdit::new(124.002, 2.031), 124.0, on_two), (124.002, 2.03), "set by hand");
    }

    #[test]
    fn edits_survive_a_new_analysis() {
        let db = LibraryDb::open_in_memory().unwrap();
        let path = Path::new("a.mp3");
        let stamp = FileStamp { size: 1, modified_ms: 1 };
        let edits = TrackEdits { grid: Some(GridEdit::imported(124.0, 0.15)), key: None };
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
