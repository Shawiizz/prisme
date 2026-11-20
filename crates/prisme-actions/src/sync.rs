//! Where two decks' beats are against each other.

use prisme_engine::DeckStatus;

/// Where a deck's beats are: its tempo and how far into its beat it is (0 to 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Beat {
    pub bpm: f64,
    pub phase: f64,
}

impl Beat {
    /// Of a deck playing on a beat grid.
    pub fn playing(deck: &DeckStatus) -> Option<Self> {
        deck.playing.then_some(Beat { bpm: deck.bpm?, phase: deck.beat_phase? })
    }
}

/// The power of two bringing `bpm` closest to `other_bpm`: 70 BPM goes with 140.
pub fn octave_ratio(bpm: f64, other_bpm: f64) -> f64 {
    (bpm / other_bpm).log2().round().exp2()
}

/// How far a deck's beats are from another deck's, in beats within [-0.5, 0.5]; ahead above 0.
pub fn phase_offset(deck: Beat, other: Beat) -> f64 {
    let other_phase = (other.phase * octave_ratio(deck.bpm, other.bpm)) % 1.0;
    let gap = deck.phase - other_phase;
    gap - gap.round()
}

/// Where a deck's beats are against another's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    In,
    Ahead,
    Behind,
    /// Not both playing on a beat grid.
    Off,
}

/// Where `deck`'s beats are against `other`'s, both playing ([`Beat::playing`]): in phase
/// within `within` beats, else ahead or behind.
pub fn phase(deck: Option<Beat>, other: Option<Beat>, within: f64) -> Phase {
    let Some(offset) = deck.zip(other).map(|(deck, other)| phase_offset(deck, other)) else {
        return Phase::Off;
    };
    if offset.abs() < within {
        Phase::In
    } else if offset > 0.0 {
        Phase::Ahead
    } else {
        Phase::Behind
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beat(bpm: f64, phase: f64) -> Beat {
        Beat { bpm, phase }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn tempos_an_octave_apart_go_together() {
        assert_eq!(octave_ratio(70.0, 140.0), 0.5);
        assert_eq!(octave_ratio(174.0, 87.0), 2.0);
        assert_eq!(octave_ratio(125.0, 128.0), 1.0);
    }

    #[test]
    fn the_offset_is_the_nearest_beat_difference() {
        assert!(close(phase_offset(beat(128.0, 0.3), beat(128.0, 0.1)), 0.2));
        assert!(close(phase_offset(beat(128.0, 0.05), beat(128.0, 0.95)), 0.1));
        assert!(close(phase_offset(beat(128.0, 0.9), beat(128.0, 0.1)), -0.2));
    }

    #[test]
    fn a_double_time_deck_is_compared_with_twice_the_other_phase() {
        assert!(close(phase_offset(beat(140.0, 0.5), beat(70.0, 0.25)), 0.0));
    }

    #[test]
    fn in_phase_ahead_or_behind_past_a_small_offset() {
        let at = |own, other| phase(Some(beat(128.0, own)), Some(beat(128.0, other)), 0.03);
        assert_eq!(at(0.51, 0.5), Phase::In);
        assert_eq!(at(0.6, 0.5), Phase::Ahead);
        assert_eq!(at(0.95, 0.05), Phase::Behind);
    }

    #[test]
    fn off_unless_both_play_on_a_grid() {
        assert_eq!(phase(None, Some(beat(128.0, 0.5)), 0.03), Phase::Off);
    }
}
