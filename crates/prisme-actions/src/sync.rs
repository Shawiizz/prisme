//! Where two decks' beats are against each other.

use prisme_engine::{DeckStatus, EngineStatus};

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

/// The tempo at which `deck` plays the beats of the deck it follows, the engine at `status`
/// (the leader, else another deck on a beat grid), an octave apart if closer: none unless both
/// have a grid.
pub fn matching_tempo(status: &EngineStatus, deck: usize) -> Option<f64> {
    let own = &status.decks[deck];
    let others = || status.decks.iter().enumerate().filter(|&(d, _)| d != deck).map(|(_, s)| s);
    let followed = others().find(|s| s.sync.leader).or_else(|| others().find(|s| s.bpm.is_some()))?;
    let (bpm, other) = (own.bpm?, followed.bpm?);
    Some(own.rate * octave_ratio(bpm, other) * other / bpm)
}

/// The tempo a deck at `status` keeps as its SYNC goes off: the one it is locked to, if it
/// plays by itself a track with a grid (a record keeps its turntable's).
pub fn kept_tempo(status: &DeckStatus) -> Option<f64> {
    (status.sync.locked && !status.vinyl.enabled && status.bpm.is_some()).then_some(status.rate)
}

/// How far to move the beat grid of a deck at `deck` (seconds of its track, a grid of
/// `grid_bpm`) for its beats to fall on `other`'s, both playing: matched by ear, the deck keeps
/// what is heard and is in phase.
pub fn grid_shift_to(deck: &DeckStatus, other: &DeckStatus, grid_bpm: f64) -> Option<f64> {
    let offset = phase_offset(Beat::playing(deck)?, Beat::playing(other)?);
    Some(offset * 60.0 / grid_bpm)
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

    #[test]
    fn a_deck_unlocked_keeps_the_tempo_it_was_locked_to_but_on_a_record() {
        let mut status = DeckStatus { bpm: Some(126.0), rate: 1.05, ..Default::default() };
        status.sync.locked = true;
        assert_eq!(kept_tempo(&status), Some(1.05));
        status.vinyl.enabled = true;
        assert_eq!(kept_tempo(&status), None, "the turntable's");
        status.vinyl.enabled = false;
        status.bpm = None;
        assert_eq!(kept_tempo(&status), None, "no grid");
    }

    #[test]
    fn a_deck_matches_the_leader_else_a_deck_on_a_grid_an_octave_apart_if_closer() {
        let mut status = EngineStatus::default();
        let deck = |bpm: f64| DeckStatus { bpm: Some(bpm), rate: 1.0, ..DeckStatus::default() };
        status.decks[..3].copy_from_slice(&[deck(125.0), deck(64.0), deck(130.0)]);
        assert!(close(matching_tempo(&status, 0).unwrap(), 128.0 / 125.0), "the first on a grid, doubled");
        status.decks[2].sync.leader = true;
        assert!(close(matching_tempo(&status, 0).unwrap(), 130.0 / 125.0), "the leader");
        status.decks[0].bpm = None;
        assert_eq!(matching_tempo(&status, 0), None, "no grid of its own");
    }

    #[test]
    fn a_grid_ahead_moves_later_by_its_offset_in_seconds_of_its_track() {
        let playing = |phase: f64| DeckStatus {
            playing: true,
            bpm: Some(120.0),
            beat_phase: Some(phase),
            ..DeckStatus::default()
        };
        let shift = grid_shift_to(&playing(0.3), &playing(0.1), 120.0).unwrap();
        assert!(close(shift, 0.1), "a fifth of a beat at 120 BPM: {shift}");
        let stopped = DeckStatus { playing: false, ..playing(0.1) };
        assert_eq!(grid_shift_to(&playing(0.3), &stopped, 120.0), None);
    }
}
