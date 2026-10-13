//! A region of the track the playhead wraps around.

use crate::grid::BeatGrid;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoopRegion {
    pub start_s: f64,
    pub end_s: f64,
}

impl LoopRegion {
    /// `beats` beats from `position_s`, or when quantizing from its nearest beat, or for a
    /// loop shorter than a beat, from its nearest step of that length: the nearest beat
    /// may be half a beat behind, the whole of a 1/16 loop with it, never reached.
    pub fn of_beats(grid: &BeatGrid, position_s: f64, beats: f64, quantize: bool) -> Self {
        let start_s = if quantize { grid.nearest_s(position_s, beats.min(1.0)) } else { position_s };
        Self { start_s, end_s: start_s + beats * grid.beat_period_s() }
    }

    pub fn length_s(&self) -> f64 {
        self.end_s - self.start_s
    }

    pub fn contains(&self, position_s: f64) -> bool {
        (self.start_s..self.end_s).contains(&position_s)
    }

    /// Same start, length multiplied by `factor`.
    pub fn scaled(&self, factor: f64) -> Self {
        Self { start_s: self.start_s, end_s: self.start_s + self.length_s() * factor }
    }

    pub fn shifted(&self, by_s: f64) -> Self {
        Self { start_s: self.start_s + by_s, end_s: self.end_s + by_s }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRID: BeatGrid = BeatGrid { bpm: 120.0, first_beat_s: 0.0 };

    #[test]
    fn a_quantized_loop_starts_on_the_nearest_beat() {
        let l = LoopRegion::of_beats(&GRID, 10.1, 4.0, true);
        assert_eq!(l, LoopRegion { start_s: 10.0, end_s: 12.0 });
        let free = LoopRegion::of_beats(&GRID, 10.1, 4.0, false);
        assert!((free.start_s - 10.1).abs() < 1e-12 && (free.length_s() - 2.0).abs() < 1e-12);
    }

    #[test]
    fn a_quantized_loop_shorter_than_a_beat_is_where_the_playhead_is() {
        // 0.4 beat past a beat (half a second each): the nearest beat is behind the
        // playhead by more than these loops are long
        let position = 10.2;
        for beats in [0.5, 0.25, 0.125, 0.0625] {
            let l = LoopRegion::of_beats(&GRID, position, beats, true);
            let reached =
                l.contains(position) || (l.start_s > position && l.start_s - position < 0.5 * l.length_s());
            assert!(reached, "{beats} beat: {l:?}");
        }
    }

    #[test]
    fn scaling_keeps_the_start() {
        let l = LoopRegion { start_s: 4.0, end_s: 6.0 };
        assert_eq!(l.scaled(0.5), LoopRegion { start_s: 4.0, end_s: 5.0 });
        assert_eq!(l.shifted(1.0), LoopRegion { start_s: 5.0, end_s: 7.0 });
        assert!(l.contains(4.0) && !l.contains(6.0));
    }
}
