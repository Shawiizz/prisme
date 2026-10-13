//! Where the beats of a track are.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeatGrid {
    pub bpm: f64,
    pub first_beat_s: f64,
}

impl BeatGrid {
    pub fn beats_at(&self, position_s: f64) -> f64 {
        (position_s - self.first_beat_s) * self.bpm / 60.0
    }

    pub fn beat_period_s(&self) -> f64 {
        60.0 / self.bpm
    }

    pub fn nearest_beat_s(&self, position_s: f64) -> f64 {
        self.nearest_s(position_s, 1.0)
    }

    /// The nearest point of the grid divided in steps of `beats` (a quarter: 0.25).
    pub fn nearest_s(&self, position_s: f64, beats: f64) -> f64 {
        let step_s = beats * self.beat_period_s();
        self.first_beat_s + ((position_s - self.first_beat_s) / step_s).round() * step_s
    }

    /// Where to land to reach `target_s` without losing the beat: the target plus how
    /// far the playhead is from its nearest beat.
    pub fn phase_preserving_target(&self, position_s: f64, target_s: f64) -> f64 {
        target_s + position_s - self.nearest_beat_s(position_s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRID: BeatGrid = BeatGrid { bpm: 120.0, first_beat_s: 0.25 };

    #[test]
    fn nearest_beat() {
        assert!((GRID.nearest_beat_s(0.95) - 0.75).abs() < 1e-12);
        assert!((GRID.nearest_beat_s(1.1) - 1.25).abs() < 1e-12);
        // the grid goes on before the first beat
        assert!((GRID.nearest_beat_s(-0.1) + 0.25).abs() < 1e-12);
    }

    #[test]
    fn a_jump_keeps_the_offset_from_the_beat() {
        // 0.1 s after a beat, jumping to a cue on a beat lands 0.1 s after it
        assert!((GRID.phase_preserving_target(0.85, 10.25) - 10.35).abs() < 1e-12);
        // 0.1 s before a beat
        assert!((GRID.phase_preserving_target(0.65, 10.25) - 10.15).abs() < 1e-12);
    }
}
