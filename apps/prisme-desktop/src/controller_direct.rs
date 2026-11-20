//! The controls that drive the engine straight from the MIDI thread, without going through
//! the UI: the jog wheels, played in time (a jog searching moves through the track quickly,
//! under shift say), and the tempo faders, moved finely. The UI hears of the tempo it then
//! shows (`deck-pitch`).

use prisme_actions::input::{Given, Input, Inputs};
use prisme_actions::scales::Tempo;
use prisme_control::{DeckAction, Target};
use prisme_engine::Command;

/// Seconds of track a turn of a jog wheel goes through in a search.
const SEARCH_SECONDS_PER_TURN: f64 = 10.0;

/// A move of a controller: which one, when on its clock, and how many steps a turn of its jog
/// wheels sends.
#[derive(Debug, Clone, Copy)]
pub struct Moved<'d> {
    pub device: &'d str,
    pub at_s: f64,
    pub jog_steps_per_turn: f64,
}

/// Turns direct actions into engine commands, each tempo fader picking its tempo up.
#[derive(Debug, Default)]
pub struct Performer {
    inputs: Inputs,
}

impl Performer {
    /// The command for `action` of `deck` driven to `value` by `moved`; `tempo` the deck's, for
    /// its fader. `None` while a fader has not picked the tempo up, and for an action not
    /// performed here.
    pub fn command(
        &mut self,
        moved: Moved,
        deck: usize,
        action: DeckAction,
        value: f32,
        tempo: Tempo,
    ) -> Option<Command> {
        let Moved { device, at_s, jog_steps_per_turn } = moved;
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
                let fader = Target::Deck(deck, action);
                match self.inputs.resolve(device, &fader, Input::Value(value), Some(tempo.position())) {
                    Some(Given::Value(position)) => {
                        Some(Command::SetPitch { deck, ratio: tempo.at(position) })
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPO: Tempo = Tempo { ratio: 1.0, range: 0.08 };

    fn moved(device: &str, at_s: f64) -> Moved<'_> {
        Moved { device, at_s, jog_steps_per_turn: 720.0 }
    }

    #[test]
    fn a_jog_turns_by_its_steps() {
        let mut performer = Performer::default();
        let turned = performer.command(moved("DDJ", 2.5), 1, DeckAction::Jog, -180.0, TEMPO);
        assert!(
            matches!(turned, Some(Command::Jog { deck: 1, turns, at_s }) if turns == -0.25 && at_s == 2.5)
        );
        let touched = performer.command(moved("DDJ", 3.0), 0, DeckAction::JogTouch, 0.2, TEMPO);
        assert!(matches!(touched, Some(Command::JogTouch { deck: 0, touched: true, .. })));
        let let_go = performer.command(moved("DDJ", 3.5), 0, DeckAction::JogTouch, 0.0, TEMPO);
        assert!(matches!(let_go, Some(Command::JogTouch { deck: 0, touched: false, .. })));
    }

    #[test]
    fn a_jog_searches_seconds_a_turn() {
        let mut performer = Performer::default();
        let searched = performer.command(moved("DDJ", 0.0), 0, DeckAction::JogSearch, 360.0, TEMPO);
        assert!(matches!(searched, Some(Command::Jump { deck: 0, seconds }) if seconds == 5.0));
    }

    fn pitch(performer: &mut Performer, device: &str, value: f32, ratio: f64) -> Option<f64> {
        let tempo = Tempo { ratio, ..TEMPO };
        match performer.command(moved(device, 0.0), 0, DeckAction::Pitch, value, tempo) {
            Some(Command::SetPitch { ratio, .. }) => Some(ratio),
            _ => None,
        }
    }

    #[test]
    fn a_tempo_fader_picks_the_tempo_up_before_moving_it() {
        let mut performer = Performer::default();
        // the screen left the tempo at +4 %: the fader at -8 % does not jump it there
        assert_eq!(pitch(&mut performer, "DDJ", 0.0, 1.04), None);
        assert_eq!(pitch(&mut performer, "DDJ", 0.5, 1.04), None);
        let picked = pitch(&mut performer, "DDJ", 0.8, 1.04).unwrap();
        assert!((picked - 1.048).abs() < 1e-6);
        assert!((pitch(&mut performer, "DDJ", 1.0, picked).unwrap() - 1.08).abs() < 1e-6);
    }

    #[test]
    fn two_controllers_on_one_deck_each_pick_its_tempo_up() {
        let mut performer = Performer::default();
        assert!(pitch(&mut performer, "DDJ", 0.5, 1.0).is_some());
        assert_eq!(pitch(&mut performer, "CDJ", 0.9, 1.0), None, "its fader is elsewhere");
        assert!(pitch(&mut performer, "DDJ", 0.55, 1.0).is_some());
    }
}
