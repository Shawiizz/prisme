//! The deck a phone's lock screen shows (Android's player): the one heard among those that go,
//! the loudest through the software mixer, else the last started; while all stand, the one heard
//! last. And when the player is told again ([`Told`]): it moves the position on by itself.

use crate::view::View;
use prisme_control::Side;
use prisme_engine::{crossfader_gain, CrossfaderCurve, MAX_DECKS};

/// Slower than this, a deck is not heard as playing: a record held or nudged, a jog let go.
/// The loads count a deck still only far below.
const PLAYING_SPEED: f64 = 0.25;
/// Two decks this close in level are as loud.
const AS_LOUD: f32 = 1e-3;
/// The player moves the position on by itself: told again past this much of a drift (s).
const DRIFT_S: f64 = 1.0;
/// A change of speed smaller than this is not told.
const SPEED_STEP: f64 = 0.005;

/// What the lock screen shows.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Shown {
    /// The decks with a track that go, a bit each.
    pub moving: u8,
    /// The deck shown: none without a track.
    pub deck: Option<usize>,
}

impl Shown {
    pub fn moves(&self, deck: usize) -> bool {
        self.moving & 1 << deck != 0
    }
}

/// When each deck last started to go, and the deck heard last.
#[derive(Debug, Clone, Default)]
pub struct Player {
    shown: Shown,
    started_at_s: [f64; MAX_DECKS],
    heard: Option<usize>,
}

impl Player {
    /// What the lock screen shows of `view` at `now_s`, the software mixer mixing (`internal`)
    /// or not.
    pub fn read(&mut self, view: &View, internal: bool, now_s: f64) -> Shown {
        let loaded = |d: usize| view.decks[d].track.is_some();
        let moves = |d: usize| {
            let status = &view.status.decks[d];
            loaded(d) && (status.playing || status.speed.abs() > PLAYING_SPEED)
        };
        let decks = 0..view.decks_in_use;
        let moving = decks.clone().filter(|&d| moves(d)).fold(0, |bits, d| bits | 1 << d);
        for d in decks.clone().filter(|&d| moving & 1 << d != 0 && !self.shown.moves(d)) {
            self.started_at_s[d] = now_s;
        }
        let later = |a: usize, b: usize| if self.started_at_s[b] > self.started_at_s[a] { b } else { a };
        let level = |d: usize| view.decks[d].volume * gain(view, d);
        let louder = |a: usize, b: usize| {
            let (la, lb) = (level(a), level(b));
            if !internal || (la - lb).abs() < AS_LOUD {
                later(a, b)
            } else if lb > la {
                b
            } else {
                a
            }
        };
        let heard = decks.clone().filter(|&d| moving & 1 << d != 0).reduce(louder);
        self.heard = heard.or(self.heard.filter(|&d| loaded(d)));
        let deck = self.heard.or_else(|| decks.clone().find(|&d| loaded(d)));
        self.shown = Shown { moving, deck };
        self.shown
    }
}

/// Where the deck shown is, and how fast it moves: 0 while it stands.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Place {
    pub position_s: f64,
    pub speed: f64,
}

/// What the player was last told, `S` what it shows but the place, and when (s).
#[derive(Debug, Clone)]
pub struct Told<S> {
    last: Option<(S, Place, f64)>,
}

impl<S> Default for Told<S> {
    fn default() -> Self {
        Self { last: None }
    }
}

impl<S: Clone + PartialEq> Told<S> {
    /// Whether the player is to be told `showing` with the deck at `place` at `now_s`: what it
    /// shows changed, or the place it moved on by itself drifted from the deck's (a jump, a
    /// loop, a scratch), or its speed changed.
    pub fn tell(&mut self, showing: &S, place: Place, now_s: f64) -> bool {
        if let Some((told, at, at_s)) = &self.last {
            let moved = at.position_s + at.speed * (now_s - at_s);
            let still =
                (at.speed - place.speed).abs() < SPEED_STEP && (moved - place.position_s).abs() < DRIFT_S;
            if told == showing && still {
                return false;
            }
        }
        self.last = Some((showing.clone(), place, now_s));
        true
    }
}

/// The crossfader's gain on `deck`.
fn gain(view: &View, deck: usize) -> f32 {
    let mixer = &view.mixer;
    let position = if mixer.crossfader_reverse { -mixer.crossfader } else { mixer.crossfader };
    let curve = if mixer.sharp_curve { CrossfaderCurve::Sharp } else { CrossfaderCurve::Smooth };
    let toward_right = (position.clamp(-1.0, 1.0) + 1.0) * 0.5;
    match view.decks[deck].side {
        Side::Left => crossfader_gain(toward_right, curve),
        Side::Right => crossfader_gain(1.0 - toward_right, curve),
        Side::Center => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::fixtures::{loaded, view};

    #[test]
    fn the_deck_heard_is_the_one_that_goes_else_the_last_heard() {
        let mut player = Player::default();
        assert_eq!(player.read(&view(), true, 0.0), Shown::default(), "nothing loaded");
        let one = loaded(loaded(view(), 0, 10.0, true), 1, 0.0, false);
        assert_eq!(player.read(&one, true, 1.0), Shown { moving: 0b01, deck: Some(0) });
        let mut stopped = one;
        stopped.status.decks[0].playing = false;
        assert_eq!(player.read(&stopped, true, 2.0).deck, Some(0), "heard last");
        let mut held = stopped;
        held.status.decks[1].speed = 0.1;
        assert_eq!(player.read(&held, true, 3.0).moving, 0, "a record held does not play");
    }

    #[test]
    fn the_player_is_told_again_past_a_drift_a_change_of_speed_or_of_what_it_shows() {
        let mut told = Told::default();
        let at = |position_s, speed| Place { position_s, speed };
        assert!(told.tell(&"one", at(10.0, 1.0), 0.0), "told first");
        assert!(!told.tell(&"one", at(12.2, 1.0), 2.0), "goes on as the player moves it");
        assert!(told.tell(&"one", at(30.0, 1.0), 2.0), "a jump");
        assert!(told.tell(&"one", at(30.0, 0.0), 2.0), "stopped");
        assert!(!told.tell(&"one", at(30.0, 0.0), 9.0));
        assert!(told.tell(&"two", at(30.0, 0.0), 9.0), "another track");
    }

    #[test]
    fn of_two_going_the_loudest_through_the_crossfader_else_the_last_started() {
        let mut player = Player::default();
        let a = loaded(loaded(view(), 0, 10.0, true), 1, 0.0, false);
        player.read(&a, true, 1.0);
        let mut both = a;
        both.status.decks[1].playing = true;
        assert_eq!(player.read(&both, true, 2.0).deck, Some(1), "as loud: the last started");
        both.decks[1].side = Side::Right;
        both.mixer.crossfader = -1.0;
        assert_eq!(player.read(&both, true, 3.0).deck, Some(0), "B faded out");
        assert_eq!(player.read(&both, false, 4.0).deck, Some(1), "a mixer of its own: the last started");
        both.mixer.crossfader_reverse = true;
        assert_eq!(player.read(&both, true, 5.0).deck, Some(1), "the sides swapped: A faded out");
    }
}
