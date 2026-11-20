//! A tempo tapped on a key.

use std::collections::VecDeque;

/// Taps further apart than this start a new count.
const RESET_S: f64 = 2.0;
/// Taps needed before a tempo is given.
const MIN_TAPS: usize = 4;
/// Only the latest taps count, so that the tempo follows a correction.
const MAX_TAPS: usize = 16;

#[derive(Debug, Clone, Default)]
pub struct TapTempo {
    taps: VecDeque<f64>,
}

impl TapTempo {
    /// A tap at `at_s` (seconds, on any clock that goes on): the tempo (BPM) once there are
    /// enough taps.
    pub fn tap(&mut self, at_s: f64) -> Option<f64> {
        if self.taps.back().is_some_and(|&last| at_s - last > RESET_S) {
            self.taps.clear();
        }
        self.taps.push_back(at_s);
        if self.taps.len() > MAX_TAPS {
            self.taps.pop_front();
        }
        let first = *self.taps.front()?;
        (self.taps.len() >= MIN_TAPS).then(|| 60.0 * (self.taps.len() - 1) as f64 / (at_s - first))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_taps_give_the_tempo() {
        let mut tap = TapTempo::default();
        assert_eq!([0.0, 0.5, 1.0].map(|s| tap.tap(s)), [None, None, None]);
        assert!((tap.tap(1.5).unwrap() - 120.0).abs() < 1e-9);
    }

    #[test]
    fn a_pause_starts_the_count_again() {
        let mut tap = TapTempo::default();
        for s in [0.0, 0.5, 1.0, 1.5] {
            tap.tap(s);
        }
        assert_eq!(tap.tap(10.0), None);
    }

    #[test]
    fn the_tempo_follows_the_latest_taps() {
        let mut tap = TapTempo::default();
        for i in 0..16 {
            tap.tap(f64::from(i) * 0.5);
        }
        let bpm = (1..=16).map(|i| tap.tap(7.5 + f64::from(i) * 0.47)).last().flatten().unwrap();
        assert!((bpm - 60.0 / 0.47).abs() < 0.05);
    }
}
