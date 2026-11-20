//! A key held: held long enough it does something else, as a controller's key with a hold does;
//! or it acts again and again while held. One rule for the controllers' keys and the screen's.

use prisme_control::{DeckAction, DeckSwitch, Target, Turn};

/// Held this long (ms), the SYNC of the screen and of the keyboard matches the leader once
/// instead of locking to it.
pub const SYNC_HOLD_MS: u32 = 450;
/// A tempo key held steps again after this wait (ms), then at this pace.
const REPEAT_WAIT_MS: u32 = 400;
const REPEAT_EVERY_MS: u32 = 60;

/// When a key held acts besides its press.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Timing {
    /// Once, held this long (ms).
    Held(u32),
    /// Again and again while held: after `wait` ms, then every `every` ms.
    Repeated { wait: u32, every: u32 },
}

/// What the interface's press of `target` does besides acting: the action a SYNC held drives,
/// or the tempo keys stepping on.
pub fn interface_rule(target: &Target) -> Option<(Timing, Option<Target>)> {
    match *target {
        Target::Deck(deck, DeckAction::Switch(DeckSwitch::Sync, Turn::Over)) => {
            Some((Timing::Held(SYNC_HOLD_MS), Some(Target::Deck(deck, DeckAction::SyncOnce))))
        }
        Target::Deck(_, DeckAction::TempoUp | DeckAction::TempoDown) => {
            Some((Timing::Repeated { wait: REPEAT_WAIT_MS, every: REPEAT_EVERY_MS }, None))
        }
        _ => None,
    }
}

/// A key pressed at a moment, on any clock that goes on (s).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pressed {
    timing: Timing,
    /// When it acts next, if it still does.
    next_s: Option<f64>,
    acted: bool,
    firing: Firing,
}

/// An action due while the key is held, performed by a clock away from the lock keeping the
/// key: a release coming meanwhile is performed after it, never before.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Firing {
    No,
    /// Due, not performed yet.
    Now,
    /// Let go meanwhile: its release waits for it.
    LetGo,
}

impl Pressed {
    pub fn new(timing: Timing, now_s: f64) -> Self {
        let wait = match timing {
            Timing::Held(ms) | Timing::Repeated { wait: ms, .. } => ms,
        };
        Self { timing, next_s: Some(now_s + f64::from(wait) / 1000.0), acted: false, firing: Firing::No }
    }

    /// Whether it acts at `now_s`: held long enough (once), or its next repeat come. Asked late
    /// (a stall), a repeat acts once and goes on at its pace from then. Acting, it fires until
    /// its action is performed ([`Self::fired`]).
    pub fn due(&mut self, now_s: f64) -> bool {
        let Some(next_s) = self.next_s.filter(|&next| now_s >= next) else { return false };
        self.next_s = match self.timing {
            Timing::Held(_) => None,
            Timing::Repeated { every, .. } => Some(next_s.max(now_s) + f64::from(every) / 1000.0),
        };
        self.acted = true;
        self.firing = Firing::Now;
        true
    }

    /// Let go: whether its release waits for its action due, not performed yet ([`Self::fired`]
    /// then says it was let go).
    pub fn let_go(&mut self) -> bool {
        if self.firing == Firing::Now {
            self.firing = Firing::LetGo;
        }
        self.released()
    }

    /// Let go already, its release waiting for its action.
    pub fn released(&self) -> bool {
        self.firing == Firing::LetGo
    }

    /// Its action due performed: whether it was let go meanwhile, its release to be performed
    /// now.
    pub fn fired(&mut self) -> bool {
        std::mem::replace(&mut self.firing, Firing::No) == Firing::LetGo
    }

    /// It acted while held: a key with a hold let go then lets its hold go, not its own action.
    pub fn acted(&self) -> bool {
        self.acted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_held_long_enough_acts_once() {
        let mut pressed = Pressed::new(Timing::Held(450), 10.0);
        assert!(!pressed.due(10.44));
        assert!(!pressed.acted(), "let go now, a tap");
        assert!(pressed.due(10.45));
        assert!(!pressed.due(11.0), "once");
        assert!(pressed.acted());
    }

    #[test]
    fn a_key_let_go_while_its_action_is_performed_is_let_go_after_it() {
        let mut pressed = Pressed::new(Timing::Held(450), 0.0);
        assert!(!pressed.let_go(), "nothing due: let go at once");
        assert!(pressed.due(0.5));
        assert!(!pressed.released());
        assert!(pressed.let_go(), "its action not performed yet");
        assert!(pressed.released());
        assert!(pressed.fired(), "performed: let go now");
        assert!(!pressed.released());
        let mut kept = Pressed::new(Timing::Held(450), 0.0);
        kept.due(0.5);
        assert!(!kept.fired(), "held on");
        assert!(!kept.let_go(), "its action performed: let go at once");
    }

    #[test]
    fn a_key_repeating_waits_then_goes_on_at_its_pace() {
        let mut pressed = Pressed::new(Timing::Repeated { wait: 400, every: 60 }, 0.0);
        let due: Vec<bool> = [0.399, 0.4, 0.459, 0.461, 0.521, 0.6, 0.6].map(|t| pressed.due(t)).into();
        assert_eq!(due, [false, true, false, true, true, true, false]);
    }

    #[test]
    fn a_repeat_asked_late_acts_once_then_keeps_its_pace() {
        let mut pressed = Pressed::new(Timing::Repeated { wait: 400, every: 60 }, 0.0);
        let due: Vec<bool> = [1.0, 1.0, 1.059, 1.06].map(|t| pressed.due(t)).into();
        assert_eq!(due, [true, false, false, true], "no burst for the repeats missed");
    }

    #[test]
    fn the_screens_sync_matches_once_held_and_its_tempo_keys_repeat() {
        let rule = |id: &str| interface_rule(&id.parse().unwrap());
        assert_eq!(
            rule("deck2.sync"),
            Some((Timing::Held(SYNC_HOLD_MS), Some("deck2.sync.once".parse().unwrap())))
        );
        assert!(matches!(rule("deck1.tempo.up"), Some((Timing::Repeated { .. }, None))));
        assert!(matches!(rule("deck1.tempo.down"), Some((Timing::Repeated { .. }, None))));
        assert_eq!(rule("deck1.sync.on"), None, "a switch turned acts at once");
        assert_eq!(rule("deck1.play"), None);
    }
}
