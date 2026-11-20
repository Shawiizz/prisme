//! The interface's keys whose press does more than act (see `prisme_actions::hold`): SYNC held
//! matches the leader once, the tempo keys step on while held. Timed on the actions' clock and
//! fired by the backend's clock, as a controller's key with a hold is.

use crate::state::lock;
use prisme_actions::hold::{interface_rule, Pressed};
use prisme_actions::screen;
use prisme_actions::view::View;
use prisme_control::{DeckAction, Target};
use std::sync::Mutex;

/// What an interface's key given a value does now.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Press {
    /// Its action, as given.
    Act,
    /// Nothing yet: held long enough, its hold acts.
    Wait,
    /// Let go before its hold: its own action, pressed and let go.
    Tap,
    /// Let go after its hold acted: the hold's action let go.
    Release(Target),
    /// Let go after its time, its hold not acted yet (the tick late): the hold's action pressed
    /// and let go.
    Held(Target),
}

/// The interface's keys held whose rule acts while they are.
#[derive(Default)]
pub(crate) struct Keys(Mutex<Vec<Held>>);

struct Held {
    target: Target,
    pressed: Pressed,
    hold: Option<Target>,
}

impl Keys {
    /// The interface's `target` given `value` at `now_s` (above 0 pressed, else let go): what
    /// to perform of it.
    pub fn press(&self, target: &Target, value: f32, now_s: f64) -> Press {
        let Some((timing, hold)) = interface_rule(target) else { return Press::Act };
        let mut held = lock(&self.0);
        let at = held.iter().position(|k| k.target == *target && !k.pressed.released());
        // let go while the clock performs its action: let go after it (see `fired`)
        if value <= 0.0 && at.is_some_and(|at| held[at].pressed.let_go()) {
            return Press::Wait;
        }
        let was = at.map(|at| held.remove(at));
        if value > 0.0 {
            let waits = hold.is_some();
            held.push(Held { target: target.clone(), pressed: Pressed::new(timing, now_s), hold });
            return if waits { Press::Wait } else { Press::Act };
        }
        let Some(hold) = hold else { return Press::Act };
        // its press lost, or never seen
        let Some(mut was) = was else { return Press::Wait };
        if was.pressed.acted() {
            Press::Release(hold)
        } else if was.pressed.due(now_s) {
            Press::Held(hold)
        } else {
            Press::Tap
        }
    }

    /// The press of `target` lost (the key gone from under the pointer): let go, nothing done.
    pub fn lost(&self, target: &Target) {
        lock(&self.0).retain(|k| k.target != *target || k.pressed.released());
    }

    /// The keys whose time came at `now_s` in `view` and the action each performs: the holds of
    /// the keys held long enough, the tempo keys stepping on, but on a deck whose tempo something
    /// else holds (sync, a record), where they stop. Each is performed, then said `fired`.
    pub fn due(&self, now_s: f64, view: &View) -> Vec<(Target, Target)> {
        let mut held = lock(&self.0);
        held.retain(|k| match k.target {
            Target::Deck(deck, DeckAction::TempoUp | DeckAction::TempoDown) => {
                deck < view.decks_in_use && !screen::deck(view, deck).tempo.held
            }
            _ => true,
        });
        held.iter_mut()
            .filter_map(|k| {
                let due = k.pressed.due(now_s);
                due.then(|| (k.target.clone(), k.hold.clone().unwrap_or_else(|| k.target.clone())))
            })
            .collect()
    }

    /// The action of the key `target` performed (see `due`): whether the key was let go
    /// meanwhile, the action to be let go now.
    pub fn fired(&self, target: &Target) -> bool {
        let mut held = lock(&self.0);
        let Some(at) = held.iter().position(|k| k.target == *target) else { return false };
        let let_go = held[at].pressed.fired();
        if let_go {
            held.remove(at);
        }
        let_go
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use prisme_engine::EngineStatus;

    /// Two decks, the first locked by sync if `synced`.
    fn seen(synced: bool) -> View {
        let state = AppState::new(prisme_session::JsonFile::in_memory(Default::default()));
        let mut status = EngineStatus::default();
        status.decks[0].sync.locked = synced;
        crate::actions::view_with(&state, status, None, false, &Default::default(), 0.0)
    }

    fn view() -> View {
        seen(false)
    }

    fn target(id: &str) -> Target {
        id.parse().unwrap()
    }

    /// What the clock performs at `now_s`: the actions due, each said performed after.
    fn fire(keys: &Keys, now_s: f64, view: &View) -> Vec<Target> {
        let due = keys.due(now_s, view);
        due.into_iter().filter(|(key, _)| !keys.fired(key)).map(|(_, action)| action).collect()
    }

    #[test]
    fn a_key_let_go_while_the_clock_performs_its_hold_is_let_go_after_it() {
        let keys = Keys::default();
        let (sync, once) = (target("deck1.sync"), target("deck1.sync.once"));
        keys.press(&sync, 1.0, 0.0);
        assert_eq!(keys.due(1.0, &view()), [(sync.clone(), once.clone())]);
        assert_eq!(keys.press(&sync, 0.0, 1.0), Press::Wait, "its hold not performed yet");
        assert_eq!(keys.press(&sync, 1.0, 1.0), Press::Wait, "pressed again meanwhile");
        assert!(keys.fired(&sync), "performed: the hold let go now");
        assert_eq!(keys.press(&sync, 0.0, 1.1), Press::Tap, "the second press its own");
        keys.press(&sync, 1.0, 2.0);
        keys.due(3.0, &view());
        keys.lost(&sync);
        assert!(!keys.fired(&sync), "lost: nothing to let go");
    }

    #[test]
    fn sync_tapped_toggles_held_matches_once() {
        let keys = Keys::default();
        let (sync, once) = (target("deck1.sync"), target("deck1.sync.once"));
        assert_eq!(keys.press(&sync, 1.0, 0.0), Press::Wait);
        assert_eq!(fire(&keys, 0.3, &view()), []);
        assert_eq!(keys.press(&sync, 0.0, 0.3), Press::Tap);
        assert_eq!(keys.press(&sync, 1.0, 1.0), Press::Wait);
        assert_eq!(fire(&keys, 1.5, &view()), std::slice::from_ref(&once));
        assert_eq!(fire(&keys, 2.0, &view()), [], "once");
        assert_eq!(keys.press(&sync, 0.0, 2.0), Press::Release(once.clone()));
        keys.press(&sync, 1.0, 3.0);
        assert_eq!(keys.press(&sync, 0.0, 3.6), Press::Held(once), "held, the tick late");
    }

    #[test]
    fn a_press_lost_does_nothing_of_its_own() {
        let keys = Keys::default();
        let sync = target("deck2.sync");
        keys.press(&sync, 1.0, 0.0);
        keys.lost(&sync);
        assert_eq!(fire(&keys, 1.0, &view()), []);
        assert_eq!(keys.press(&sync, 0.0, 1.0), Press::Wait, "nothing to let go");
    }

    #[test]
    fn a_tempo_key_steps_at_once_then_on_while_held() {
        let keys = Keys::default();
        let up = target("deck1.tempo.up");
        assert_eq!(keys.press(&up, 1.0, 0.0), Press::Act);
        assert_eq!(fire(&keys, 0.4, &view()), std::slice::from_ref(&up));
        assert_eq!(fire(&keys, 0.46, &view()), std::slice::from_ref(&up));
        assert_eq!(keys.press(&up, 0.0, 0.5), Press::Act);
        assert_eq!(fire(&keys, 1.0, &view()), []);
    }

    #[test]
    fn a_tempo_key_stops_once_something_else_holds_the_tempo() {
        let keys = Keys::default();
        let down = target("deck1.tempo.down");
        keys.press(&down, 1.0, 0.0);
        let synced = seen(true);
        assert!(screen::deck(&synced, 0).tempo.held, "the fixture's deck locked by sync");
        assert_eq!(fire(&keys, 0.5, &synced), []);
        assert_eq!(fire(&keys, 0.6, &view()), [], "its release never came: it does not go on later");
    }
}
