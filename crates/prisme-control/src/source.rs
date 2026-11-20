//! What a value a profile shows on a controller is (its outputs' and displays' `source`): a
//! deck's track measured, or the position of an action's fader.

use crate::target::{number, Target, Unknown, MAX_DECKS};
use std::fmt;
use std::str::FromStr;

/// What is measured of a deck's track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Measure {
    /// Its place in the track, 0 to 1.
    Position,
    /// The turn of its record at 33⅓, 0 to 1.
    Turn,
    Bpm,
    /// Seconds played.
    Elapsed,
    /// Seconds left.
    Remaining,
    /// Seconds in all.
    Duration,
    /// Its tempo change, in percent.
    Tempo,
    /// The range of its tempo fader, in percent.
    TempoRange,
}

impl Measure {
    const ALL: [(Measure, &'static str); 8] = [
        (Measure::Position, "position"),
        (Measure::Turn, "turn"),
        (Measure::Bpm, "bpm"),
        (Measure::Elapsed, "elapsed"),
        (Measure::Remaining, "remaining"),
        (Measure::Duration, "duration"),
        (Measure::Tempo, "tempo"),
        (Measure::TempoRange, "tempoRange"),
    ];

    fn id(self) -> &'static str {
        Self::ALL.iter().find(|(m, _)| *m == self).map_or("", |(_, id)| id)
    }
}

/// What a value shown on a controller is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Source {
    /// Of the track of a deck, by its place (0 for deck A).
    Measured(usize, Measure),
    /// Where the fader of an action stands, 0 to 1.
    Position(Target),
}

impl FromStr for Source {
    type Err = Unknown;

    /// A deck's measure first: `deck1.position` and `deck1.tempo` are actions too.
    fn from_str(id: &str) -> Result<Self, Unknown> {
        let measured = id.strip_prefix("deck").and_then(|rest| {
            let (deck, measure) = rest.split_once('.')?;
            let measure = Measure::ALL.iter().find(|(_, name)| *name == measure)?.0;
            Some(Source::Measured(number(deck, MAX_DECKS)?, measure))
        });
        match measured {
            Some(source) => Ok(source),
            None => id.parse().map(Source::Position),
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::Measured(deck, measure) => write!(f, "deck{}.{}", deck + 1, measure.id()),
            Source::Position(target) => target.fmt(f),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::{Band, DeckAction, MixerAction};

    #[test]
    fn a_source_is_a_deck_measured_or_a_fader() {
        let read = |id: &str| id.parse::<Source>().ok();
        assert_eq!(read("deck1.position"), Some(Source::Measured(0, Measure::Position)));
        assert_eq!(read("deck4.tempoRange"), Some(Source::Measured(3, Measure::TempoRange)));
        assert_eq!(read("deck2.tempo"), Some(Source::Measured(1, Measure::Tempo)));
        assert_eq!(read("deck2.eq.low"), Some(Source::Position(Target::Deck(1, DeckAction::Eq(Band::Low)))));
        assert_eq!(read("mixer.main"), Some(Source::Position(Target::Mixer(MixerAction::Main))));
        for unknown in ["deck5.bpm", "deck0.turn", "deck01.turn", "deck+1.turn", "deck1.speed", "main"] {
            assert_eq!(read(unknown), None, "{unknown}");
        }
        for id in ["deck3.remaining", "deck1.fx2.amount"] {
            assert_eq!(read(id).unwrap().to_string(), id);
        }
    }
}
