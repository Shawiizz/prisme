//! What a deck's performance pads play: the lengths of its rolls and of its fader cuts, and its
//! slicer, the eight beats from the start of a bar, each a slice a pad plays round and round
//! while held, the track going on underneath.

use prisme_control::{CUTS, ROLLS, SLICES};

/// The rolls' lengths, in beats, by their place among the rolls.
pub const ROLL_BEATS: [f64; ROLLS] = [0.0625, 0.125, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0];
/// The fader cuts' lengths, in beats.
pub const CUT_BEATS: [f64; CUTS] = [0.0625, 0.125, 0.25, 0.5];
const BEATS_PER_BAR: f64 = 4.0;

/// Where the slicer of a track at `position_s` starts: the bar it is in, by a grid whose first
/// beat is at `first_beat_s`, a beat lasting `beat_s`.
pub fn bar_start(position_s: f64, first_beat_s: f64, beat_s: f64) -> f64 {
    let bar_s = BEATS_PER_BAR * beat_s;
    first_beat_s + ((position_s - first_beat_s) / bar_s).floor() * bar_s
}

/// The slice of the slicer starting at `start_s` a track at `position_s` is in, if it is in it,
/// a beat lasting `beat_s`.
pub fn slice_at(start_s: f64, position_s: f64, beat_s: f64) -> Option<usize> {
    let slice = ((position_s - start_s) / beat_s).floor();
    (slice >= 0.0 && slice < SLICES as f64).then_some(slice as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slicer_starts_on_the_bar_the_track_is_in_and_holds_eight_beats() {
        // 120 BPM from 0.1 s: a bar of 2 s
        assert!((bar_start(5.3, 0.1, 0.5) - 4.1).abs() < 1e-9);
        assert!((bar_start(0.05, 0.1, 0.5) + 1.9).abs() < 1e-9, "before the first beat");
        assert_eq!(slice_at(4.1, 4.1, 0.5), Some(0));
        assert_eq!(slice_at(4.1, 7.0, 0.5), Some(5));
        assert_eq!(slice_at(4.1, 8.1, 0.5), None, "past its eight beats");
        assert_eq!(slice_at(4.1, 4.0, 0.5), None);
    }
}
