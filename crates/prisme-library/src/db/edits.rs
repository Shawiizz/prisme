//! Corrections made by hand to a track's beat grid and key. They are kept apart from the
//! analysis results, so that analysing the file again does not undo them.

use super::{path_key, DbError, LibraryDb};
use prisme_analysis::{octave_ratio, Key, KeyEstimate, Mode, Tempo, TrackAnalysis};
use prisme_grid::{Anchor, BeatGrid};
use rusqlite::{params, OptionalExtension, Row};
use std::ops::RangeInclusive;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct GridEdit {
    /// One tempo throughout, or one that changes between anchors (a grid imported from another
    /// program); its bars sure.
    pub grid: BeatGrid,
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

    /// One tempo throughout, bar 1 starting at `first_beat_s`.
    pub fn new(bpm: f64, first_beat_s: f64) -> Self {
        Self { grid: BeatGrid::steady(bpm, first_beat_s, true), beats_from_analysis: false }
    }

    /// Another program's grid: see [`GridEdit::beats_from_analysis`].
    pub fn imported(grid: BeatGrid) -> Self {
        Self { grid: grid.with_bars_sure(true), beats_from_analysis: true }
    }

    /// One tempo throughout, `bpm`, from its first beat; `None` outside
    /// [`GridEdit::BPM_RANGE`].
    pub fn with_bpm(self, bpm: f64) -> Option<Self> {
        Self::BPM_RANGE.contains(&bpm).then(|| self.set(|g| g.with_bpm(bpm)))
    }

    /// Its tempo times `factor` (2: twice as many beats); `None` if it would leave
    /// [`GridEdit::BPM_RANGE`].
    pub fn scaled(self, factor: f64) -> Option<Self> {
        Self::BPM_RANGE.contains(&(self.grid.mean_bpm() * factor)).then(|| self.set(|g| g.scaled(factor)))
    }

    /// The grid moved by `seconds` (later when positive), its bars with it.
    pub fn shifted(self, seconds: f64) -> Self {
        self.set(|g| g.shifted(seconds))
    }

    /// The grid moved the least so that a beat falls at `position_s`, its bars with it.
    pub fn with_beat_at(self, position_s: f64) -> Self {
        self.set(|g| g.with_beat_at(position_s))
    }

    /// The grid moved the least so that a beat falls at `position_s`, a bar starting on it: bar
    /// 1 the bar's nearest the one before.
    pub fn with_bar_at(self, position_s: f64) -> Self {
        self.set(|g| g.with_bar_at(position_s))
    }

    /// The grid with its beat nearest `position_s` pinned there (see [`BeatGrid::pinned`]);
    /// `None` where the pin cannot go.
    pub fn pinned_at(self, position_s: f64) -> Option<Self> {
        let grid = self.grid.pinned(position_s)?;
        Some(self.set(|_| grid))
    }

    /// The grid without its pin nearest `position_s` (see [`BeatGrid::unpinned`]); `None`
    /// without pins.
    pub fn unpinned_at(self, position_s: f64) -> Option<Self> {
        let grid = self.grid.unpinned(position_s)?;
        Some(self.set(|_| grid))
    }

    /// The grid as `change` leaves it, set by hand: the beats no longer the analysis's.
    fn set(self, change: impl FnOnce(BeatGrid) -> BeatGrid) -> Self {
        Self { grid: change(self.grid), beats_from_analysis: false }
    }

    /// An imported grid moved the least onto the beats of `analysed` in the middle of the track
    /// (`duration_s` long), if it is the same tempo there and its beats fall near them: its
    /// tempo and bars stay. Any other grid as it is.
    fn on_beats_of(&self, analysed: Option<&BeatGrid>, duration_s: Option<f64>) -> BeatGrid {
        let grid = self.grid.clone();
        let Some(analysed) = analysed.filter(|_| self.beats_from_analysis) else { return grid };
        let middle = duration_s.map_or(grid.first_beat_s(), |d| d / 2.0);
        let (bpm, theirs_bpm) = (grid.bpm_at(middle), analysed.bpm_at(middle));
        let octave = octave_ratio(bpm, theirs_bpm);
        if (bpm / octave / theirs_bpm - 1.0).abs() > SAME_TEMPO {
            return grid;
        }
        let own = grid.nearest_beat_s(middle);
        let theirs = analysed.nearest_beat_s(middle);
        // the beats of the slower grid are beats of the faster one too
        let period = grid.beat_period_s_at(middle).min(analysed.beat_period_s_at(middle));
        let gap = theirs - own - ((theirs - own) / period).round() * period;
        if gap.abs() > SAME_BEATS_S {
            return grid;
        }
        grid.shifted(gap)
    }
}

impl From<&BeatGrid> for GridEdit {
    fn from(grid: &BeatGrid) -> Self {
        Self { grid: grid.clone().with_bars_sure(true), beats_from_analysis: false }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackEdits {
    pub grid: Option<GridEdit>,
    pub key: Option<Key>,
}

impl TrackEdits {
    pub fn is_empty(&self) -> bool {
        self.grid.is_none() && self.key.is_none()
    }

    /// The grid of a track `duration_s` long analysed as `detected`, with the corrections.
    pub fn beat_grid(&self, detected: Option<BeatGrid>, duration_s: Option<f64>) -> Option<BeatGrid> {
        match &self.grid {
            Some(edit) => Some(edit.on_beats_of(detected.as_ref(), duration_s)),
            None => detected,
        }
    }

    /// The analysis of a track `duration_s` long with the corrections applied: a grid whose
    /// tempo changes at its tempo on average.
    pub fn apply(&self, detected: &TrackAnalysis, duration_s: Option<f64>) -> TrackAnalysis {
        let edited = self.grid.as_ref().and_then(|_| self.beat_grid(detected.beat_grid(), duration_s));
        TrackAnalysis {
            tempo: match &edited {
                Some(grid) => Some(Tempo {
                    bpm: grid.mean_bpm(),
                    first_beat_s: grid.first_beat_s(),
                    confidence: 1.0,
                    bars_sure: true,
                }),
                None if self.grid.is_some() => None,
                None => detected.tempo,
            },
            anchors: match &edited {
                Some(grid) => (!grid.is_steady()).then(|| grid.anchors().into()),
                None => detected.anchors.clone(),
            },
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
        let bpm = r.get::<_, Option<f64>>("edit_bpm")?;
        let first_beat_s = r.get::<_, Option<f64>>("edit_first_beat_s")?;
        let anchors = r.get::<_, Option<Vec<u8>>>("edit_anchors")?;
        let grid = match (bpm, first_beat_s) {
            (Some(bpm), Some(first_beat_s)) => Some(GridEdit {
                grid: anchors
                    .and_then(|blob| BeatGrid::anchored(anchors_of(&blob), bpm, true))
                    .unwrap_or(BeatGrid::steady(bpm, first_beat_s, true)),
                beats_from_analysis: r.get("edit_beats_from_analysis")?,
            }),
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

/// Anchors as stored: each its beat then its instant, little-endian `f64`s.
pub(super) fn anchors_blob(anchors: &[Anchor]) -> Vec<u8> {
    anchors.iter().flat_map(|a| [a.beat.to_le_bytes(), a.time_s.to_le_bytes()]).flatten().collect()
}

pub(super) fn anchors_of(blob: &[u8]) -> Vec<Anchor> {
    let number = |bytes: &[u8]| f64::from_le_bytes(bytes.try_into().unwrap_or_default());
    blob.chunks_exact(16).map(|a| Anchor { beat: number(&a[..8]), time_s: number(&a[8..]) }).collect()
}

impl LibraryDb {
    pub fn edits(&self, path: &Path) -> Result<TrackEdits, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT bpm AS edit_bpm, first_beat_s AS edit_first_beat_s, anchors AS edit_anchors,
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
        let grid = edits.grid.as_ref();
        conn.prepare_cached(
            "INSERT OR REPLACE INTO track_edits (path, bpm, first_beat_s, anchors, beats_from_analysis,
                 key_tonic, key_minor)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?
        .execute(params![
            path_key(path),
            grid.map(|g| g.grid.bpm()),
            grid.map(|g| g.grid.first_beat_s()),
            grid.filter(|g| !g.grid.is_steady()).map(|g| anchors_blob(g.grid.anchors())),
            grid.is_some_and(|g| g.beats_from_analysis),
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
            anchors: None,
        }
    }

    /// 120 BPM from beat 0 at 1 s to beat 8 at 5 s, then 96 BPM to beat 16 at 10 s, 100 after.
    fn drifting() -> BeatGrid {
        let anchors = vec![
            Anchor { beat: 0.0, time_s: 1.0 },
            Anchor { beat: 8.0, time_s: 5.0 },
            Anchor { beat: 16.0, time_s: 10.0 },
        ];
        BeatGrid::anchored(anchors, 100.0, true).unwrap()
    }

    #[test]
    fn grid_corrections() {
        let grid = GridEdit::new(120.0, 0.1);
        assert_eq!(grid.clone().with_bpm(240.0), Some(GridEdit::new(240.0, 0.1)));
        assert_eq!(grid.clone().with_bpm(20.0), None);
        assert_eq!(grid.clone().scaled(0.5).map(|g| g.grid.bpm()), Some(60.0));
        assert_eq!(grid.clone().scaled(4.0), None);
        let first = |g: GridEdit| g.grid.first_beat_s();
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert!(close(first(grid.clone().shifted(0.05)), 0.15));
        assert!(close(first(grid.clone().shifted(-0.15)), -0.05), "the bars move with the beats");
        assert!(close(first(grid.clone().with_beat_at(10.3)), 0.3));
        assert!(close(first(grid.with_bar_at(11.3)), -0.7), "half way between two bars 1, the earlier");
        let imported = GridEdit::imported(drifting());
        assert!(!imported.shifted(0.1).beats_from_analysis, "moved by hand, the beats are its own");
    }

    #[test]
    fn edits_override_the_detected_values() {
        let edits =
            TrackEdits { grid: Some(GridEdit::new(124.0, 0.15)), key: Some(Key::new(0, Mode::Major)) };
        let applied = edits.apply(&detected(), Some(100.0));
        assert_eq!(
            applied.tempo,
            Some(Tempo { bpm: 124.0, first_beat_s: 0.15, confidence: 1.0, bars_sure: true })
        );
        let key = applied.key.unwrap();
        assert_eq!((key.key, key.tuning_cents), (Key::new(0, Mode::Major), -8.0));
        assert_eq!(TrackEdits::default().apply(&detected(), Some(100.0)), detected());
    }

    #[test]
    fn an_imported_grid_keeps_its_tempo_and_bars_on_the_analysed_beats() {
        let applied = |grid: BeatGrid, bpm: f64, first_beat_s: f64| {
            let detected = Some(BeatGrid::steady(bpm, first_beat_s, true));
            let edits = TrackEdits { grid: Some(GridEdit::imported(grid)), key: None };
            let grid = edits.beat_grid(detected, Some(200.0)).unwrap();
            (grid.bpm(), (grid.first_beat_s() * 100.0).round() / 100.0)
        };
        let imported = BeatGrid::steady(124.002, 2.031, true);
        // the analysis's beats on 2 s, four beats before it
        let on_two = 2.0 - 4.0 * 60.0 / 124.0;
        assert_eq!(applied(imported.clone(), 124.0, on_two), (124.002, 2.0), "on the beats, bar 1 kept");
        assert_eq!(applied(imported.clone(), 62.0, on_two), (124.002, 2.0), "an octave apart");
        assert_eq!(applied(imported.clone(), 248.0, on_two), (124.002, 2.0), "the other way");
        let off_beats = on_two + 30.0 / 124.0;
        assert_eq!(applied(imported.clone(), 124.0, off_beats), (124.002, 2.03), "on other beats");
        assert_eq!(applied(imported.clone(), 126.0, on_two), (124.002, 2.03), "another tempo");
        let by_hand = TrackEdits { grid: Some(GridEdit::new(124.002, 2.031)), key: None };
        let analysed = Some(BeatGrid::steady(124.0, on_two, true));
        assert_eq!(by_hand.beat_grid(analysed, Some(200.0)).unwrap().first_beat_s(), 2.031, "set by hand");
    }

    #[test]
    fn an_imported_grid_whose_tempo_changes_is_laid_on_the_analysed_beats_where_it_is_the_same() {
        // the middle of a 15 s track, at 96 BPM on the imported grid; the analysis 0.03 s earlier
        let analysed = drifting().shifted(-0.03);
        let middle = analysed.nearest_beat_s(7.5);
        let detected = Some(Tempo { bpm: 96.0, first_beat_s: middle, confidence: 0.5, bars_sure: true });
        let edits = TrackEdits { grid: Some(GridEdit::imported(drifting())), key: None };
        let grid = edits.beat_grid(Some(BeatGrid::steady(96.0, middle, true)), Some(15.0)).unwrap();
        assert!((grid.first_beat_s() - 0.97).abs() < 1e-9, "moved, its anchors with it");
        assert_eq!(grid.anchors().len(), 3);
        let tempo = edits
            .apply(&TrackAnalysis { tempo: detected, ..TrackAnalysis::default() }, Some(15.0))
            .tempo
            .unwrap();
        assert!((tempo.bpm - drifting().mean_bpm()).abs() < 1e-9, "shown at its tempo on average");
    }

    #[test]
    fn edits_survive_a_new_analysis() {
        let db = LibraryDb::open_in_memory().unwrap();
        let path = Path::new("a.mp3");
        let stamp = FileStamp { size: 1, modified_ms: 1 };
        let edits = TrackEdits { grid: Some(GridEdit::imported(drifting())), key: None };
        db.save(&TrackRecord::analyzed(path, stamp, 100.0, detected(), TrackTags::default())).unwrap();
        db.save_edits(path, &edits).unwrap();
        db.save(&TrackRecord::analyzed(path, stamp, 100.0, detected(), TrackTags::default())).unwrap();

        let record = db.get(path).unwrap().unwrap();
        assert_eq!(record.edits, edits, "its anchors kept");
        assert_eq!(db.edits(path).unwrap(), edits);
        let steady = TrackEdits { grid: Some(GridEdit::new(124.0, 0.15)), key: None };
        db.save_edits(path, &steady).unwrap();
        assert_eq!(db.get(path).unwrap().unwrap().edits, steady);
        assert_eq!(db.get(path).unwrap().unwrap().effective_analysis().tempo.unwrap().bpm, 124.0);

        db.save_edits(path, &TrackEdits::default()).unwrap();
        assert_eq!(db.get(path).unwrap().unwrap().edits, TrackEdits::default());
    }
}
