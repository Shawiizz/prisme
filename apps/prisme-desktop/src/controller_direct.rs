//! The controls that drive the engine straight from the MIDI thread, without going through
//! the UI: the jog wheels, played in time (a jog searching moves through the track quickly,
//! under shift say), and the tempo faders, moved finely. The UI hears of the tempo it then
//! shows (`deck-pitch`).

use prisme_control::{DeckAction, Pickup};
use prisme_engine::Command;

/// Seconds of track a turn of a jog wheel goes through in a search.
const SEARCH_SECONDS_PER_TURN: f64 = 10.0;

/// What a deck's tempo fader acts on: the tempo now and the fader's range.
#[derive(Debug, Clone, Copy)]
pub struct Tempo {
    pub ratio: f64,
    pub range: f64,
}

impl Tempo {
    /// The fader position of the tempo (0-1, the middle being the track's own tempo).
    fn position(self) -> f32 {
        (0.5 + (self.ratio - 1.0) / (2.0 * self.range)).clamp(0.0, 1.0) as f32
    }

    fn at(self, position: f32) -> f64 {
        1.0 + (f64::from(position) * 2.0 - 1.0) * self.range
    }
}

/// Turns direct actions into engine commands, with the pickup of each tempo fader.
#[derive(Debug, Default)]
pub struct Performer {
    pitch_pickups: Vec<Pickup>,
}

impl Performer {
    /// The command for `action` of `deck` driven to `value` at `at_s` on the clock of a
    /// controller whose jog wheels turn by `jog_steps_per_turn`; `tempo` the deck's, for its
    /// fader. `None` while a fader has not picked the tempo up, and for an action not performed
    /// here.
    pub fn command(
        &mut self,
        deck: usize,
        action: DeckAction,
        value: f32,
        at_s: f64,
        jog_steps_per_turn: f64,
        tempo: Tempo,
    ) -> Option<Command> {
        match action {
            DeckAction::Jog => {
                Some(Command::Jog { deck, turns: f64::from(value) / jog_steps_per_turn, at_s })
            }
            // however lightly: some wheels tell how hard they are touched
            DeckAction::JogTouch => Some(Command::JogTouch { deck, touched: value > 0.0, at_s }),
            DeckAction::JogSearch => Some(Command::Jump {
                deck,
                seconds: f64::from(value) / jog_steps_per_turn * SEARCH_SECONDS_PER_TURN,
            }),
            DeckAction::Pitch => {
                if self.pitch_pickups.len() <= deck {
                    self.pitch_pickups.resize_with(deck + 1, Pickup::default);
                }
                self.pitch_pickups[deck]
                    .takes(value, tempo.position())
                    .then(|| Command::SetPitch { deck, ratio: tempo.at(value) })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPO: Tempo = Tempo { ratio: 1.0, range: 0.08 };
    const STEPS: f64 = 720.0;

    #[test]
    fn a_jog_turns_by_its_steps() {
        let mut performer = Performer::default();
        let turned = performer.command(1, DeckAction::Jog, -180.0, 2.5, STEPS, TEMPO);
        assert!(
            matches!(turned, Some(Command::Jog { deck: 1, turns, at_s }) if turns == -0.25 && at_s == 2.5)
        );
        let touched = performer.command(0, DeckAction::JogTouch, 0.2, 3.0, STEPS, TEMPO);
        assert!(matches!(touched, Some(Command::JogTouch { deck: 0, touched: true, .. })));
        let let_go = performer.command(0, DeckAction::JogTouch, 0.0, 3.5, STEPS, TEMPO);
        assert!(matches!(let_go, Some(Command::JogTouch { deck: 0, touched: false, .. })));
    }

    #[test]
    fn a_jog_searches_seconds_a_turn() {
        let mut performer = Performer::default();
        let searched = performer.command(0, DeckAction::JogSearch, 360.0, 0.0, STEPS, TEMPO);
        assert!(matches!(searched, Some(Command::Jump { deck: 0, seconds }) if seconds == 5.0));
    }

    #[test]
    fn a_tempo_fader_picks_the_tempo_up_before_moving_it() {
        let mut performer = Performer::default();
        let pitch = |p: &mut Performer, value, ratio| match p.command(
            0,
            DeckAction::Pitch,
            value,
            0.0,
            STEPS,
            Tempo { ratio, ..TEMPO },
        ) {
            Some(Command::SetPitch { ratio, .. }) => Some(ratio),
            _ => None,
        };
        // the screen left the tempo at +4 %: the fader at -8 % does not jump it there
        assert_eq!(pitch(&mut performer, 0.0, 1.04), None);
        assert_eq!(pitch(&mut performer, 0.5, 1.04), None);
        let picked = pitch(&mut performer, 0.8, 1.04).unwrap();
        assert!((picked - 1.048).abs() < 1e-6);
        assert!((pitch(&mut performer, 1.0, picked).unwrap() - 1.08).abs() < 1e-6);
    }
}
