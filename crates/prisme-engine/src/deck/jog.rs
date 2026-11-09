//! A controller's jog wheel. Touched, it holds the track like a hand on a record and moves
//! it as it turns (scratching, through [`super::hand`]); turned by its edge while the track
//! plays, it pushes or holds it back for a moment (nudging); turned while stopped, it moves
//! the track. A record in vinyl control drives the deck: the jog does nothing then.

/// Track time in one turn of a jog wheel: one turn of a record at 33⅓ rpm.
pub(crate) const TURN_S: f64 = 1.8;
/// Speed added by a turn of the edge (+100 % for a whole turn), fading away.
const NUDGE_PER_TURN: f64 = 1.0;
/// How long a nudge takes to fade to a third (exponential decay).
const NUDGE_FADE_S: f64 = 0.1;
const MAX_NUDGE: f64 = 0.5;

#[derive(Debug, Default)]
pub(crate) struct Jog {
    /// While touched: where the hand has taken the track (s).
    held_at_s: Option<f64>,
    /// Speed added by the edge, fading (0.1 = +10 %).
    pub(crate) nudge: f64,
}

/// What a turn of the jog does to the deck.
pub(crate) enum Turn {
    /// The hand holding the track moved it there.
    Hand(f64),
    /// Moves the stopped track by this much (s).
    Move(f64),
    Nothing,
}

impl Jog {
    /// Touched with the track at `position_s` (then it follows the hand), or let go.
    pub(crate) fn touch(&mut self, position_s: Option<f64>) {
        self.held_at_s = position_s;
    }

    pub(crate) fn touched(&self) -> bool {
        self.held_at_s.is_some()
    }

    /// Turned by `turns` (positive forwards), the track playing or not.
    pub(crate) fn turn(&mut self, turns: f64, playing: bool) -> Turn {
        if let Some(held) = self.held_at_s.as_mut() {
            *held += turns * TURN_S;
            Turn::Hand(*held)
        } else if playing {
            self.nudge = (self.nudge + turns * NUDGE_PER_TURN).clamp(-MAX_NUDGE, MAX_NUDGE);
            Turn::Nothing
        } else {
            Turn::Move(turns * TURN_S)
        }
    }

    /// The nudge fades over `seconds`.
    pub(crate) fn fade(&mut self, seconds: f64) {
        self.nudge *= (-seconds / NUDGE_FADE_S).exp();
        if self.nudge.abs() < 1e-6 {
            self.nudge = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touched_it_carries_the_track() {
        let mut jog = Jog::default();
        jog.touch(Some(10.0));
        assert!(matches!(jog.turn(0.5, true), Turn::Hand(p) if (p - 10.9).abs() < 1e-9));
        assert!(matches!(jog.turn(-1.0, true), Turn::Hand(p) if (p - 9.1).abs() < 1e-9));
        jog.touch(None);
        assert!(!jog.touched());
    }

    #[test]
    fn its_edge_nudges_a_playing_track_for_a_moment() {
        let mut jog = Jog::default();
        assert!(matches!(jog.turn(0.1, true), Turn::Nothing));
        assert!((jog.nudge - 0.1).abs() < 1e-9);
        jog.fade(NUDGE_FADE_S);
        assert!((jog.nudge - 0.1 / std::f64::consts::E).abs() < 1e-9);
        jog.turn(10.0, true);
        assert_eq!(jog.nudge, MAX_NUDGE);
    }

    #[test]
    fn it_moves_a_stopped_track() {
        let mut jog = Jog::default();
        assert!(matches!(jog.turn(-0.25, false), Turn::Move(s) if (s + 0.45).abs() < 1e-9));
        assert_eq!(jog.nudge, 0.0);
    }
}
