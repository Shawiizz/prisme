//! The controls that drive the engine straight from the MIDI thread, without going through
//! the UI: the jog wheels, played in time, and the tempo faders, moved finely. The UI hears
//! of the tempo it then shows (`deck-pitch`).

use prisme_control::Pickup;
use prisme_engine::Command;

/// Steps in a turn of a jog wheel. Most controllers send hundreds; the exact number only
/// scales scratches and nudges, and is measured on each model when its preset is made.
const JOG_STEPS_PER_TURN: f64 = 720.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direct {
    Jog(usize),
    JogTouch(usize),
    Pitch(usize),
}

impl Direct {
    /// The direct action named `target` (`deck1.jog`, `deck2.pitch`…), if it is one.
    pub fn of(target: &str) -> Option<Self> {
        let (deck, action) = target.strip_prefix("deck")?.split_once('.')?;
        let deck = deck.parse::<usize>().ok()?.checked_sub(1)?;
        match action {
            "jog" => Some(Direct::Jog(deck)),
            "jog.touch" => Some(Direct::JogTouch(deck)),
            "pitch" => Some(Direct::Pitch(deck)),
            _ => None,
        }
    }
}

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
    /// The command for `action` driven to `value` at `at_s` on the controller's clock;
    /// `tempo` the deck's, for its fader. `None` while a fader has not picked the tempo up.
    pub fn command(&mut self, action: Direct, value: f32, at_s: f64, tempo: Tempo) -> Option<Command> {
        match action {
            Direct::Jog(deck) => {
                Some(Command::Jog { deck, turns: f64::from(value) / JOG_STEPS_PER_TURN, at_s })
            }
            Direct::JogTouch(deck) => Some(Command::JogTouch { deck, touched: value > 0.5, at_s }),
            Direct::Pitch(deck) => {
                if self.pitch_pickups.len() <= deck {
                    self.pitch_pickups.resize_with(deck + 1, Pickup::default);
                }
                self.pitch_pickups[deck]
                    .takes(value, tempo.position())
                    .then(|| Command::SetPitch { deck, ratio: tempo.at(value) })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPO: Tempo = Tempo { ratio: 1.0, range: 0.08 };

    #[test]
    fn direct_actions_are_the_jogs_and_the_tempo_faders() {
        assert_eq!(Direct::of("deck1.jog"), Some(Direct::Jog(0)));
        assert_eq!(Direct::of("deck2.jog.touch"), Some(Direct::JogTouch(1)));
        assert_eq!(Direct::of("deck2.pitch"), Some(Direct::Pitch(1)));
        assert_eq!(Direct::of("deck1.play"), None);
        assert_eq!(Direct::of("deck0.jog"), None);
        assert_eq!(Direct::of("mixer.crossfader"), None);
    }

    #[test]
    fn a_jog_turns_by_its_steps() {
        let mut performer = Performer::default();
        let turned = performer.command(Direct::Jog(1), -180.0, 2.5, TEMPO);
        assert!(
            matches!(turned, Some(Command::Jog { deck: 1, turns, at_s }) if turns == -0.25 && at_s == 2.5)
        );
        let touched = performer.command(Direct::JogTouch(0), 1.0, 3.0, TEMPO);
        assert!(matches!(touched, Some(Command::JogTouch { deck: 0, touched: true, .. })));
    }

    #[test]
    fn a_tempo_fader_picks_the_tempo_up_before_moving_it() {
        let mut performer = Performer::default();
        let pitch = |p: &mut Performer, value, ratio| match p.command(
            Direct::Pitch(0),
            value,
            0.0,
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
