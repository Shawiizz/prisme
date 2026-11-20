//! What a deck's performance pads play: the lengths of its loops, rolls and fader cuts, and its
//! slicer, the eight beats from the start of a bar, each a slice a pad plays round and round
//! while held, the track going on underneath.

use prisme_control::{CUTS, ROLLS, SLICES};
use prisme_engine::LoopStatus;

/// The rolls' lengths, in beats, by their place among the rolls.
pub const ROLL_BEATS: [f64; ROLLS] = [0.0625, 0.125, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0];
/// The fader cuts' lengths, in beats.
pub const CUT_BEATS: [f64; CUTS] = [0.0625, 0.125, 0.25, 0.5];
const BEATS_PER_BAR: f64 = 4.0;
/// A loop this close to a length (of a beat, or of the deck's size) has it.
const LOOP_LENGTH_WITHIN: f64 = 0.01;
/// The lengths the loops are named by: whole 32nds of a beat, each further than twice
/// [`LOOP_LENGTH_WITHIN`] from the next, so that a loop has one at most.
const NAMED_LENGTH: f64 = 1.0 / 32.0;

/// Whether a deck plays (`looping`) a loop of `beats`, on its grid: the key of that loop
/// leaves it, its light is lit.
pub fn plays_loop(looping: &LoopStatus, beats: f64) -> bool {
    looping.active && looping.beats.is_some_and(|playing| (playing - beats).abs() < LOOP_LENGTH_WITHIN)
}

/// The beats of the loop a deck plays (`looping`), on its grid, by the length it has (see
/// [`plays_loop`]) if it has one of those loops are named by: what the screen's pads show.
pub fn playing_loop(looping: &LoopStatus) -> Option<f64> {
    let beats = looping.beats.filter(|_| looping.active)?;
    let named = (beats / NAMED_LENGTH).round() * NAMED_LENGTH;
    Some(if (beats - named).abs() < LOOP_LENGTH_WITHIN { named } else { beats })
}

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

    #[test]
    fn a_loop_has_a_length_within_a_hundredth_of_a_beat_on_a_grid() {
        let looping = |beats: Option<f64>| LoopStatus { beats, active: true, ..LoopStatus::default() };
        assert!(plays_loop(&looping(Some(4.0)), 4.0));
        assert!(plays_loop(&looping(Some(8.005)), 8.0));
        assert!(!plays_loop(&looping(Some(8.05)), 8.0), "within 1 %, not within a hundredth");
        assert!(!plays_loop(&looping(Some(4.0)), 2.0));
        assert!(!plays_loop(&looping(None), 4.0), "no grid");
        let stored = LoopStatus { active: false, ..looping(Some(4.0)) };
        assert!(!plays_loop(&stored, 4.0), "stored, not playing");
    }

    #[test]
    fn the_pads_are_shown_the_length_a_loop_has_by_the_same_rule() {
        let looping = |beats: f64| LoopStatus { beats: Some(beats), active: true, ..LoopStatus::default() };
        for beats in [8.005, 7.995, 0.25, 0.0625, 3.0] {
            let named = playing_loop(&looping(beats)).unwrap();
            assert!(plays_loop(&looping(beats), named), "{beats}");
            assert_eq!(named, (beats * 32.0).round() / 32.0, "{beats}");
        }
        assert_eq!(playing_loop(&looping(8.05)), Some(8.05), "no length a pad has");
        assert_eq!(playing_loop(&LoopStatus { active: false, ..looping(4.0) }), None);
    }
}
