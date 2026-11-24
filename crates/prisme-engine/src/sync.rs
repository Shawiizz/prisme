//! Tempo and beat synchronisation of follower decks to a leader deck.
//!
//! Internal play is locked to the leader: tempo, and phase held by a small speed
//! correction. With a record in relative mode the phase stays in the DJ's hands: it is
//! only snapped when sync is engaged, never corrected afterwards. Absolute mode has no
//! sync, the record decides where the track is.

use crate::command::Rpm;
use crate::grid::{BeatGrid, BEATS_PER_BAR};
use crate::tempo::PlatterTempo;

const SPEED_SMOOTHING_S: f64 = 0.3;
/// Slow enough to absorb a pitch fader move, fast enough to let a nudge through.
const PLATTER_REFERENCE_SMOOTHING_S: f64 = 2.0;
/// A platter speed further than this from its recent average is a gesture, and only a
/// speed held this long counts as tempo (a scratch stroke is shorter).
const PLATTER_STEADY_TOLERANCE: f64 = 0.05;
const PLATTER_STEADY_MIN_S: f64 = 0.3;
const PLATTER_MIN_SPEED: f64 = 0.5;
const PHASE_GAIN: f64 = 0.2;
const PHASE_MAX_CORRECTION: f64 = 0.01;
const RESNAP_BEATS: f64 = 0.08;
pub(crate) const LEADER_MIN_SPEED: f64 = 0.2;
/// How long a leader resumed after an audio restart is waited for to lead again, its speed
/// rising from nothing, its record read afresh.
pub(crate) const LEADER_RESUME_S: f64 = 1.0;

#[derive(Debug, Clone, Copy)]
pub(crate) struct LeaderBeat {
    pub(crate) bpm: f64,
    /// Beats since its grid's first, which starts a bar.
    pub(crate) beats: f64,
    pub(crate) bars_sure: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SyncTarget {
    pub(crate) rate: f64,
    /// Follower beats minus leader beats, in `[-0.5, 0.5]`.
    pub(crate) phase_error: f64,
    /// The same within half a bar, `[-2, 2]`, when both know where their bars start: what
    /// engaging sync snaps away, the follower's bars on the leader's (as Serato's beat sync,
    /// Pioneer's bar sync). Once locked, only the beats are held, a beat jump let be.
    pub(crate) bar_error: f64,
}

/// Matches an octave of the leader's tempo when that is closer (70 BPM follows 140).
/// The beat the followers lock to: the leader's own, except while it loops, when a clock
/// carries on at its tempo: the jumps back of a loop shorter than a beat would otherwise
/// shift the followers' phase at every turn of it.
#[derive(Default)]
pub(crate) struct LeaderClock {
    /// The leader, and its beat at the start of the next block.
    next: Option<(usize, f64)>,
}

impl LeaderClock {
    /// The beat of `deck`, leading, at the start of a block of `seconds`; `actual` is its
    /// own.
    pub(crate) fn beat(
        &mut self,
        deck: usize,
        actual: LeaderBeat,
        looping: bool,
        seconds: f64,
    ) -> LeaderBeat {
        let beats = match self.next {
            Some((leader, beats)) if leader == deck && looping => beats,
            _ => actual.beats,
        };
        self.next = Some((deck, beats + actual.bpm / 60.0 * seconds));
        LeaderBeat { beats, ..actual }
    }
}

pub(crate) fn sync_target(own: &BeatGrid, own_position_s: f64, leader: LeaderBeat) -> SyncTarget {
    let octave = (own.bpm / leader.bpm).log2().round().exp2();
    let beats = own.beats_at(own_position_s) - octave * leader.beats;
    let phase_error = beats - beats.round();
    let bars = octave == 1.0 && own.bars_sure && leader.bars_sure;
    let bar = f64::from(BEATS_PER_BAR);
    let bar_error = if bars { beats - bar * (beats / bar).round() } else { phase_error };
    SyncTarget { rate: octave * leader.bpm / own.bpm, phase_error, bar_error }
}

pub(crate) fn snap_offset_s(own: &BeatGrid, phase_error: f64) -> f64 {
    -phase_error * own.beat_period_s()
}

pub(crate) fn phase_correction(phase_error: f64) -> f64 {
    (-phase_error * PHASE_GAIN).clamp(-PHASE_MAX_CORRECTION, PHASE_MAX_CORRECTION)
}

pub(crate) fn needs_resnap(phase_error: f64) -> bool {
    phase_error.abs() > RESNAP_BEATS
}

pub(crate) struct DeckSync {
    pub(crate) locked: bool,
    pub(crate) once_pending: bool,
    pub(crate) snap_pending: bool,
    /// Relative vinyl: factor applied to the platter speed.
    pub(crate) rate_offset: f64,
    /// Rate a locked internal deck keeps while it has no leader.
    pub(crate) held_rate: Option<f64>,
    pub(crate) smoothed_speed: f64,
    platter_recent: f64,
    platter_steady_s: f64,
    /// Steady platter speed, pitch fader included, gestures excluded: what relative
    /// locked play measures the gestures against.
    pub(crate) platter_reference: f64,
    /// The tempo of the record: what is shown and what followers match.
    pub(crate) tempo: PlatterTempo,
}

impl DeckSync {
    pub(crate) fn new() -> Self {
        Self {
            locked: false,
            once_pending: false,
            snap_pending: false,
            rate_offset: 1.0,
            held_rate: None,
            smoothed_speed: 0.0,
            platter_recent: 0.0,
            platter_steady_s: 0.0,
            platter_reference: 1.0,
            tempo: PlatterTempo::new(Rpm::default().turn_s()),
        }
    }

    /// Drops the sync settings, keeps what was learnt about the speeds.
    pub(crate) fn reset(&mut self) {
        self.locked = false;
        self.once_pending = false;
        self.snap_pending = false;
        self.rate_offset = 1.0;
        self.held_rate = None;
    }

    pub(crate) fn observe(&mut self, speed: f64, platter_speed: Option<f64>, seconds: f64) {
        self.smoothed_speed += (speed - self.smoothed_speed) * smoothing(seconds, SPEED_SMOOTHING_S);
        if let Some(p) = platter_speed {
            if self.platter_recent < PLATTER_MIN_SPEED {
                // a platter that starts turning: no history to average with
                self.platter_recent = p;
            }
            let steady =
                p > PLATTER_MIN_SPEED && (p - self.platter_recent).abs() < PLATTER_STEADY_TOLERANCE * p;
            self.platter_steady_s = if steady { self.platter_steady_s + seconds } else { 0.0 };
            self.platter_recent += (p - self.platter_recent) * smoothing(seconds, SPEED_SMOOTHING_S);
            if self.platter_steady_s >= PLATTER_STEADY_MIN_S {
                self.platter_reference +=
                    (p - self.platter_reference) * smoothing(seconds, PLATTER_REFERENCE_SMOOTHING_S);
                self.tempo.observe(p, seconds);
            }
        }
    }

    /// The platter's current steady speed, if it turns.
    pub(crate) fn platter_now(&self) -> Option<f64> {
        (self.platter_recent > PLATTER_MIN_SPEED).then_some(self.platter_recent)
    }
}

fn smoothing(seconds: f64, time_constant_s: f64) -> f64 {
    1.0 - (-seconds / time_constant_s).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tempos_an_octave_apart_are_matched_by_octave() {
        let leader = LeaderBeat { bpm: 140.0, beats: 10.0, bars_sure: true };
        let half = BeatGrid { bpm: 70.0, first_beat_s: 0.0, bars_sure: true };
        assert!((sync_target(&half, 0.0, leader).rate - 1.0).abs() < 1e-12);
        let close = BeatGrid { bpm: 128.0, first_beat_s: 0.0, bars_sure: true };
        assert!((sync_target(&close, 0.0, leader).rate - 140.0 / 128.0).abs() < 1e-12);
    }

    #[test]
    fn phase_error_is_the_nearest_beat_difference() {
        let grid = BeatGrid { bpm: 120.0, first_beat_s: 0.0, bars_sure: true };
        let leader = LeaderBeat { bpm: 120.0, beats: 7.0, bars_sure: true };
        assert!((sync_target(&grid, 3.1, leader).phase_error - 0.2).abs() < 1e-9);
        assert!((sync_target(&grid, 2.9, leader).phase_error + 0.2).abs() < 1e-9);
        assert!((snap_offset_s(&grid, 0.2) + 0.1).abs() < 1e-12);
    }

    #[test]
    fn engaging_lays_the_bars_on_the_leaders_when_both_know_them() {
        // the leader on the fourth beat of a bar (beat 7), the deck on the second (beat 5.2)
        let grid = BeatGrid { bpm: 120.0, first_beat_s: 0.0, bars_sure: true };
        let leader = LeaderBeat { bpm: 120.0, beats: 7.0, bars_sure: true };
        let target = sync_target(&grid, 2.6, leader);
        assert!((target.phase_error - 0.2).abs() < 1e-9, "held once locked: the beats");
        assert!((target.bar_error + 1.8).abs() < 1e-9, "snapped on engaging: two beats on, less 0.2");
        let unsure = BeatGrid { bars_sure: false, ..grid };
        assert!((sync_target(&unsure, 2.6, leader).bar_error - 0.2).abs() < 1e-9, "the beats only");
        let leader_unsure = LeaderBeat { bars_sure: false, ..leader };
        assert!((sync_target(&grid, 2.6, leader_unsure).bar_error - 0.2).abs() < 1e-9);
        let double = BeatGrid { bpm: 240.0, ..grid };
        assert_eq!(
            sync_target(&double, 1.3, leader).bar_error,
            sync_target(&double, 1.3, leader).phase_error
        );
    }

    #[test]
    fn a_steady_platter_moves_the_reference_but_gestures_do_not() {
        let mut s = DeckSync::new();
        for _ in 0..2000 {
            s.observe(1.35, Some(1.35), 0.01);
        }
        assert!((s.platter_reference - 1.35).abs() < 0.001, "even far from 33 rpm");
        for i in 0..100 {
            let scratch = if i % 10 < 5 { 2.0 } else { -2.0 };
            s.observe(scratch, Some(scratch), 0.01);
        }
        assert!((s.platter_reference - 1.35).abs() < 0.001);
        assert!((s.tempo.rate() - 1.35).abs() < 0.001);
    }
}
