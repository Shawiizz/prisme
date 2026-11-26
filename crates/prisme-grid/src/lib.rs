//! Where the beats of a track are.
//!
//! A track made to be mixed keeps one tempo: its grid is that tempo from bar 1's first beat. A
//! track played live drifts: its grid pins beats to instants (anchors), the tempo straight
//! between two of them, as the grids Serato and rekordbox keep of such tracks. Beats are
//! counted from bar 1's first beat (0), negative before it.
//!
//! Nothing here allocates once a grid is made: the audio thread reads grids.

use std::sync::Arc;

/// The beats of a bar, its first on the grid's first beat.
pub const BEATS_PER_BAR: u32 = 4;

/// The slowest and the fastest tempo a stretch between two anchors may have (BPM): beyond, the
/// anchors are not a grid.
const TEMPO_RANGE: std::ops::RangeInclusive<f64> = 20.0..=400.0;

/// A beat pinned to an instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    /// Counted from bar 1's first beat.
    pub beat: f64,
    pub time_s: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BeatGrid {
    /// The tempo throughout, or past the last anchor.
    bpm: f64,
    /// Bar 1's first beat.
    first_beat_s: f64,
    bars_sure: bool,
    /// Where the tempo changes, two at least, in order; none for one tempo throughout.
    anchors: Option<Arc<[Anchor]>>,
}

impl BeatGrid {
    /// One tempo throughout, bar 1 starting at `first_beat_s`.
    pub const fn steady(bpm: f64, first_beat_s: f64, bars_sure: bool) -> Self {
        Self { bpm, first_beat_s, bars_sure, anchors: None }
    }

    /// A tempo straight between each two `anchors` (two at least, their beats and instants
    /// rising), `bpm` past the last, the first stretch's before the first. `None` if they do
    /// not make a grid.
    pub fn anchored(anchors: Vec<Anchor>, bpm: f64, bars_sure: bool) -> Option<Self> {
        let finite = anchors.iter().all(|a| a.beat.is_finite() && a.time_s.is_finite());
        let rising = anchors.windows(2).all(|w| w[1].beat > w[0].beat && w[1].time_s > w[0].time_s);
        let tempos = anchors.windows(2).all(|w| TEMPO_RANGE.contains(&stretch_bpm(w[0], w[1])));
        if anchors.len() < 2 || !finite || !rising || !tempos || !TEMPO_RANGE.contains(&bpm) {
            return None;
        }
        let mut grid = Self { bpm, first_beat_s: 0.0, bars_sure, anchors: Some(anchors.into()) };
        grid.first_beat_s = grid.time_of(0.0);
        Some(grid)
    }

    /// Whether it can be played on: a tempo and an instant that are numbers, the tempo above 0.
    pub fn is_valid(&self) -> bool {
        self.bpm.is_finite() && self.bpm > 0.0 && self.first_beat_s.is_finite()
    }

    /// The tempo throughout, or past the last anchor.
    pub fn bpm(&self) -> f64 {
        self.bpm
    }

    /// The tempo on average from the first anchor to the last, or throughout: the one a
    /// library shows.
    pub fn mean_bpm(&self) -> f64 {
        match self.anchors() {
            [first, .., last] => stretch_bpm(*first, *last),
            _ => self.bpm,
        }
    }

    /// Bar 1's first beat.
    pub fn first_beat_s(&self) -> f64 {
        self.first_beat_s
    }

    /// Whether its first beat surely starts a bar: SYNC lays its bars on the leader's, else
    /// only its beats.
    pub fn bars_sure(&self) -> bool {
        self.bars_sure
    }

    pub fn with_bars_sure(self, bars_sure: bool) -> Self {
        Self { bars_sure, ..self }
    }

    /// Where the tempo changes: none for one tempo throughout.
    pub fn anchors(&self) -> &[Anchor] {
        self.anchors.as_deref().unwrap_or_default()
    }

    pub fn is_steady(&self) -> bool {
        self.anchors.is_none()
    }

    /// The beats from bar 1's first to `time_s` (fractional, negative before it).
    pub fn beats_at(&self, time_s: f64) -> f64 {
        let (from, beats_per_s) = self.stretch_at_time(time_s);
        from.beat + (time_s - from.time_s) * beats_per_s
    }

    /// Where beat `beats` (fractional) falls.
    pub fn time_of(&self, beats: f64) -> f64 {
        let (from, beats_per_s) = self.stretch_at_beat(beats);
        from.time_s + (beats - from.beat) / beats_per_s
    }

    /// The tempo at `time_s`.
    pub fn bpm_at(&self, time_s: f64) -> f64 {
        self.stretch_at_time(time_s).1 * 60.0
    }

    /// How long the beat at `time_s` lasts.
    pub fn beat_period_s_at(&self, time_s: f64) -> f64 {
        60.0 / self.bpm_at(time_s)
    }

    pub fn nearest_beat_s(&self, time_s: f64) -> f64 {
        self.nearest_s(time_s, 1.0)
    }

    /// The nearest point of the grid divided in steps of `beats` (a quarter: 0.25).
    pub fn nearest_s(&self, time_s: f64, beats: f64) -> f64 {
        self.time_of((self.beats_at(time_s) / beats).round() * beats)
    }

    /// Where the bar `time_s` is in starts (before the first beat, a bar counted back).
    pub fn bar_start_s(&self, time_s: f64) -> f64 {
        let bar = f64::from(BEATS_PER_BAR);
        self.time_of((self.beats_at(time_s) / bar).floor() * bar)
    }

    /// Where the bar after the one `time_s` is in starts.
    pub fn next_bar_s(&self, time_s: f64) -> f64 {
        let bar = f64::from(BEATS_PER_BAR);
        self.time_of(((self.beats_at(time_s) / bar).floor() + 1.0) * bar)
    }

    /// Where to land to reach `target_s` without losing the beat: as far from the target's
    /// nearest beat as the playhead is from its own.
    pub fn phase_preserving_target(&self, position_s: f64, target_s: f64) -> f64 {
        let beats = self.beats_at(position_s);
        self.time_of(self.beats_at(target_s).round() + beats - beats.round())
    }

    /// The grid moved by `seconds` (later when positive), its bars with it.
    pub fn shifted(self, seconds: f64) -> Self {
        let anchors = self.anchors.as_deref().map(|anchors| {
            anchors.iter().map(|a| Anchor { time_s: a.time_s + seconds, ..*a }).collect::<Arc<[Anchor]>>()
        });
        Self { first_beat_s: self.first_beat_s + seconds, anchors, ..self }
    }

    /// The grid moved the least so that a beat falls at `time_s`, its bars with it.
    pub fn with_beat_at(self, time_s: f64) -> Self {
        let beats = self.beats_at(time_s);
        let late = (beats - beats.round()) * self.beat_period_s_at(time_s);
        self.shifted(late)
    }

    /// The grid moved the least so that a beat falls at `time_s`, a bar starting on it: bar 1
    /// the bar's nearest the one before.
    pub fn with_bar_at(self, time_s: f64) -> Self {
        let grid = self.with_beat_at(time_s);
        let beat = grid.beats_at(time_s).round();
        let bar = f64::from(BEATS_PER_BAR);
        grid.renumbered(beat + (-beat / bar).round() * bar)
    }

    /// One tempo throughout, `bpm`, from its first beat.
    pub fn with_bpm(self, bpm: f64) -> Self {
        Self::steady(bpm, self.first_beat_s, self.bars_sure)
    }

    /// Its tempo times `factor` (2: twice as many beats), bar 1's first beat kept.
    pub fn scaled(self, factor: f64) -> Self {
        let anchors = self.anchors.as_deref().map(|anchors| {
            anchors.iter().map(|a| Anchor { beat: a.beat * factor, ..*a }).collect::<Arc<[Anchor]>>()
        });
        Self { bpm: self.bpm * factor, anchors, ..self }
    }

    /// The grid with its beat nearest `time_s` pinned there, as a DJ follows a track whose tempo
    /// drifts: the beats between it and the pins either side stretch or squeeze to fit, a
    /// steady grid pinned from bar 1's first beat, where it is. Past the last pin, the tempo goes
    /// on as it went before it. `None` where the pin cannot go: not after the pin before it, or
    /// not before the one after, or with a tempo out of reach between them.
    pub fn pinned(&self, time_s: f64) -> Option<Self> {
        let beat = self.beats_at(time_s).round();
        let mut anchors = match self.anchors() {
            [] => vec![Anchor { beat: 0.0, time_s: self.first_beat_s }],
            anchors => anchors.to_vec(),
        };
        anchors.retain(|a| a.beat != beat);
        let at = anchors.partition_point(|a| a.beat < beat);
        anchors.insert(at, Anchor { beat, time_s });
        let bpm = match anchors.as_slice() {
            [only] => return Some(Self::through(*only, self.bpm, self.bars_sure)),
            [.., before, last] if at == anchors.len() - 1 => stretch_bpm(*before, *last),
            _ => self.bpm,
        };
        Self::anchored(anchors, bpm, self.bars_sure)
    }

    /// The grid without its pin nearest `time_s`: past the pin before it, the tempo goes on as it
    /// went up to the pin taken off; with one pin left, one tempo throughout, the one between the
    /// two. `None` without pins.
    pub fn unpinned(&self, time_s: f64) -> Option<Self> {
        let anchors = self.anchors();
        let distance = |a: &Anchor| (a.time_s - time_s).abs();
        let nearest =
            (0..anchors.len()).min_by(|&a, &b| distance(&anchors[a]).total_cmp(&distance(&anchors[b])))?;
        let taken = anchors[nearest];
        let mut kept = anchors.to_vec();
        kept.remove(nearest);
        match kept.as_slice() {
            [only] => {
                let bpm = stretch_bpm(*only, taken);
                TEMPO_RANGE.contains(&bpm).then(|| Self::through(*only, bpm, self.bars_sure))
            }
            [.., last] if nearest == kept.len() => {
                let bpm = stretch_bpm(*last, taken);
                Self::anchored(kept, bpm, self.bars_sure)
            }
            _ => Self::anchored(kept, self.bpm, self.bars_sure),
        }
    }

    /// One tempo throughout, `bpm`, through the beat and the instant of `anchor`.
    fn through(anchor: Anchor, bpm: f64, bars_sure: bool) -> Self {
        Self::steady(bpm, anchor.time_s - anchor.beat * 60.0 / bpm, bars_sure)
    }

    /// The beats counted from beat `beat`, which becomes bar 1's first.
    fn renumbered(self, beat: f64) -> Self {
        let first_beat_s = self.time_of(beat);
        let anchors = self.anchors.as_deref().map(|anchors| {
            anchors.iter().map(|a| Anchor { beat: a.beat - beat, ..*a }).collect::<Arc<[Anchor]>>()
        });
        Self { first_beat_s, anchors, ..self }
    }

    /// The anchor a stretch starts from, and its beats a second, at `time_s`.
    fn stretch_at_time(&self, time_s: f64) -> (Anchor, f64) {
        self.stretch(self.anchors().partition_point(|a| a.time_s <= time_s))
    }

    /// The anchor a stretch starts from, and its beats a second, at beat `beats`.
    fn stretch_at_beat(&self, beats: f64) -> (Anchor, f64) {
        self.stretch(self.anchors().partition_point(|a| a.beat <= beats))
    }

    /// The stretch before anchor `after` (the first stretch before the first anchor, the
    /// grid's tempo past the last).
    fn stretch(&self, after: usize) -> (Anchor, f64) {
        match self.anchors() {
            [] => (Anchor { beat: 0.0, time_s: self.first_beat_s }, self.bpm / 60.0),
            anchors if after >= anchors.len() => (anchors[anchors.len() - 1], self.bpm / 60.0),
            anchors => {
                let i = after.saturating_sub(1).min(anchors.len() - 2);
                (anchors[i], stretch_bpm(anchors[i], anchors[i + 1]) / 60.0)
            }
        }
    }
}

/// The tempo from one anchor to the next.
fn stretch_bpm(from: Anchor, to: Anchor) -> f64 {
    (to.beat - from.beat) / (to.time_s - from.time_s) * 60.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEADY: BeatGrid = BeatGrid::steady(120.0, 0.25, true);

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
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
    fn nearest_beat() {
        assert!(close(STEADY.nearest_beat_s(0.95), 0.75));
        assert!(close(STEADY.nearest_beat_s(1.1), 1.25));
        assert!(close(STEADY.nearest_beat_s(-0.1), -0.25), "the grid goes on before the first beat");
    }

    #[test]
    fn a_jump_keeps_the_offset_from_the_beat() {
        // 0.1 s after a beat, jumping to a cue on a beat lands 0.1 s after it
        assert!(close(STEADY.phase_preserving_target(0.85, 10.25), 10.35));
        // 0.1 s before a beat
        assert!(close(STEADY.phase_preserving_target(0.65, 10.25), 10.15));
    }

    #[test]
    fn a_drifting_grid_has_its_tempo_between_each_two_anchors() {
        let grid = drifting();
        assert!(close(grid.first_beat_s(), 1.0));
        assert!(close(grid.beats_at(3.0), 4.0));
        assert!(close(grid.beats_at(7.5), 12.0));
        assert!(close(grid.beats_at(13.0), 21.0), "past the last anchor, its own tempo");
        assert!(close(grid.beats_at(0.0), -2.0), "before the first, the first stretch's");
        for beats in [-3.0, 0.0, 4.5, 8.0, 12.25, 16.0, 30.0] {
            assert!(close(grid.beats_at(grid.time_of(beats)), beats), "{beats}");
        }
        assert!(close(grid.bpm_at(2.0), 120.0));
        assert!(close(grid.bpm_at(6.0), 96.0));
        assert!(close(grid.bpm_at(11.0), 100.0));
        assert!(close(grid.mean_bpm(), 16.0 / 9.0 * 60.0));
    }

    #[test]
    fn a_drifting_grid_counts_its_bars_and_beats_on_its_own_tempo() {
        let grid = drifting();
        assert!(close(grid.nearest_beat_s(5.4), 5.0 + 0.625), "a beat lasts 0.625 s at 96 BPM");
        assert!(close(grid.bar_start_s(6.0), 5.0));
        assert!(close(grid.next_bar_s(6.0), 7.5));
        assert!(close(grid.next_bar_s(4.0), 5.0), "the bar after the last of the first stretch");
    }

    #[test]
    fn a_grid_moved_by_hand_keeps_its_anchors_with_it() {
        let grid = drifting().shifted(0.5);
        assert!(close(grid.first_beat_s(), 1.5));
        assert!(close(grid.beats_at(5.5), 8.0));
        let beat_there = drifting().with_beat_at(5.2);
        assert!(close(beat_there.beats_at(5.2), 8.0), "moved the least, a beat falls there");
        let bar_there = drifting().with_bar_at(6.25);
        assert!(close(bar_there.beats_at(6.25), 12.0), "beat 10 starts a bar now");
        assert!(close(bar_there.first_beat_s(), 0.0), "bar 1 the nearest the one before: 2 beats back");
        assert!(close(bar_there.bpm_at(6.25), 96.0), "the tempo kept");
        let doubled = drifting().scaled(2.0);
        assert!(close(doubled.beats_at(5.0), 16.0));
        assert!(close(doubled.bpm_at(11.0), 200.0));
        assert!(drifting().with_bpm(124.0).is_steady(), "a tempo set by hand holds throughout");
    }

    #[test]
    fn steady_corrections_are_those_of_a_single_tempo() {
        let grid = BeatGrid::steady(120.0, 0.1, true);
        assert!(close(grid.clone().shifted(0.05).first_beat_s(), 0.15));
        assert!(close(grid.clone().with_beat_at(10.3).first_beat_s(), 0.3));
        assert!(close(grid.clone().with_beat_at(10.05).first_beat_s(), 0.05), "moved the least: earlier");
        assert!(close(grid.clone().with_bar_at(10.3).first_beat_s(), 0.3), "bar 1 stays the nearest");
        assert!(close(grid.with_bar_at(11.3).first_beat_s(), -0.7), "half way between two, the earlier");
    }

    #[test]
    fn anchors_that_are_not_a_grid_are_refused() {
        let at = |beat, time_s| Anchor { beat, time_s };
        assert!(BeatGrid::anchored(vec![at(0.0, 1.0)], 120.0, true).is_none(), "one alone");
        assert!(
            BeatGrid::anchored(vec![at(0.0, 1.0), at(4.0, 1.0)], 120.0, true).is_none(),
            "no time between"
        );
        assert!(BeatGrid::anchored(vec![at(4.0, 1.0), at(0.0, 3.0)], 120.0, true).is_none(), "backwards");
        assert!(BeatGrid::anchored(vec![at(0.0, 0.0), at(1.0, 60.0)], 120.0, true).is_none(), "1 BPM");
        assert!(BeatGrid::anchored(vec![at(0.0, 0.0), at(4.0, 2.0)], 0.0, true).is_none(), "no tempo after");
    }

    #[test]
    fn a_drifting_track_is_followed_pin_by_pin() {
        // 120 BPM from 1 s; the kick of beat 16 comes 0.1 s late, that of beat 32 0.2 s late
        let grid = BeatGrid::steady(120.0, 1.0, true);
        let once = grid.pinned(9.1).unwrap();
        assert_eq!(once.anchors(), [Anchor { beat: 0.0, time_s: 1.0 }, Anchor { beat: 16.0, time_s: 9.1 }]);
        assert!(close(once.time_of(16.0), 9.1) && close(once.first_beat_s(), 1.0), "bar 1 stays");
        assert!(close(once.bpm_at(20.0), once.bpm_at(5.0)), "past the pin, the tempo it had before");
        let twice = once.pinned(17.25).unwrap();
        assert!(close(twice.time_of(32.0), 17.25) && close(twice.time_of(16.0), 9.1));
        let moved = twice.pinned(9.05).unwrap();
        assert_eq!(moved.anchors().len(), 3, "a beat pinned again moves its pin");
        assert!(close(moved.time_of(16.0), 9.05) && close(moved.time_of(32.0), 17.25));
        assert!(once.pinned(1.0 + 0.5).is_some(), "between two pins");
        assert!(BeatGrid::steady(120.0, 1.0, true).pinned(1.02).unwrap().is_steady(), "bar 1 pinned: moved");
    }

    #[test]
    fn a_pin_next_to_another_squeezes_one_beat() {
        let grid = BeatGrid::steady(120.0, 1.0, true).pinned(9.0).unwrap();
        // beat 17 is due at 9.5 s; its kick a quarter of a beat early
        let squeezed = grid.pinned(9.37).unwrap();
        assert!(close(squeezed.time_of(17.0), 9.37) && close(squeezed.time_of(16.0), 9.0));
        assert!(close(squeezed.bpm_at(9.2), 60.0 / 0.37), "the beat between them shorter");
    }

    #[test]
    fn pins_come_off_one_by_one_back_to_one_tempo() {
        let grid = BeatGrid::steady(120.0, 1.0, true).pinned(9.1).unwrap().pinned(17.25).unwrap();
        let fewer = grid.unpinned(17.0).unwrap();
        assert_eq!(fewer.anchors().len(), 2);
        assert!(close(fewer.time_of(16.0), 9.1));
        assert!(close(fewer.time_of(32.0), 17.25), "past the pin before, as it went to the one taken off");
        let steady = fewer.unpinned(9.0).unwrap();
        assert!(steady.is_steady());
        assert!(
            close(steady.time_of(0.0), 1.0) && close(steady.time_of(16.0), 9.1),
            "the tempo between the two"
        );
        assert!(steady.unpinned(5.0).is_none(), "no pin left");
    }
}
