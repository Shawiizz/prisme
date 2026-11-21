//! A CDJ's cue key: pressed while the track plays, back to the cue point and stopped; pressed
//! on the cue point, the track plays while it is held; pressed elsewhere, the cue point is set
//! there. A record turning plays the track, whatever the deck's own play.

use crate::view::plays;
use prisme_engine::DeckStatus;

/// A deck this close to its cue point (s) is on it.
const CUE_TOLERANCE_S: f64 = 0.02;

/// Whether a track at `position_s` is on its cue point at `cue_s`.
pub fn at_cue(position_s: f64, cue_s: f64) -> bool {
    (position_s - cue_s).abs() < CUE_TOLERANCE_S
}

/// What the cue key pressed does to a deck.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CuePress {
    /// Back to the cue point, stopped.
    Back,
    /// Played from the cue point while held.
    Preview,
    /// The cue point set where the track stands.
    Set(f64),
}

/// Whether a deck at `status` plays, for its cue keys: by itself, or its record turning it.
pub fn playing(status: &DeckStatus) -> bool {
    status.playing || (status.vinyl.enabled && plays(status))
}

/// The cue key pressed on a deck at `status`, its cue point at `cue_s`: nothing without a track.
pub fn pressed(status: &DeckStatus, cue_s: f64) -> Option<CuePress> {
    Some(if !status.loaded {
        return None;
    } else if playing(status) {
        CuePress::Back
    } else if at_cue(status.position_s, cue_s) {
        CuePress::Preview
    } else {
        CuePress::Set(status.position_s)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deck(position_s: f64, playing: bool) -> DeckStatus {
        DeckStatus { loaded: true, position_s, playing, ..Default::default() }
    }

    #[test]
    fn the_cue_key_goes_back_previews_or_sets_the_cue_point() {
        assert_eq!(pressed(&deck(40.0, true), 12.0), Some(CuePress::Back));
        assert_eq!(pressed(&deck(12.01, false), 12.0), Some(CuePress::Preview));
        assert_eq!(pressed(&deck(30.0, false), 12.0), Some(CuePress::Set(30.0)));
        assert_eq!(pressed(&DeckStatus::default(), 0.0), None, "no track");
    }

    #[test]
    fn a_record_turning_plays_the_track_for_the_cue_key() {
        let mut turning = deck(30.0, false);
        turning.speed = 1.0;
        assert_eq!(
            pressed(&turning, 12.0),
            Some(CuePress::Set(30.0)),
            "a hand moving a deck playing by itself"
        );
        turning.vinyl.enabled = true;
        assert_eq!(pressed(&turning, 12.0), Some(CuePress::Back));
        turning.speed = 0.0;
        assert_eq!(pressed(&turning, 12.0), Some(CuePress::Set(30.0)), "the record stopped");
    }
}
