//! Where an action is performed: at once on the thread that asks for it (the engine's
//! commands, what is kept in memory), or in turn on one worker, for those that read or write
//! the disk or the library or decode a file: a press and its release keep their order.

use prisme_control::{DeckAction, GeneralAction, LibraryAction, SampleAction, Target};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Instant,
    Slow,
}

pub fn lane(target: &Target) -> Lane {
    match target {
        // a cue or a beat grid written to the library, a sound decoded, a track ejected
        Target::Deck(
            _,
            DeckAction::Cue
            | DeckAction::CueSet
            | DeckAction::HotCue(_)
            | DeckAction::HotCueDelete(_)
            | DeckAction::GridHere
            | DeckAction::GridEarlier
            | DeckAction::GridLater
            | DeckAction::GridFaster
            | DeckAction::GridSlower
            | DeckAction::GridMove
            | DeckAction::Tap
            | DeckAction::Eject
            | DeckAction::Sample(_, SampleAction::Load | SampleAction::Smart),
        )
        | Target::Library(
            LibraryAction::Load(_)
            | LibraryAction::LoadPlay(_)
            | LibraryAction::LoadFree
            | LibraryAction::Double(_)
            | LibraryAction::Go
            | LibraryAction::Preview,
        )
        | Target::General(GeneralAction::Record) => Lane::Slow,
        _ => Lane::Instant,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_reads_or_writes_the_disk_waits_its_turn_the_rest_is_done_at_once() {
        let of = |id: &str| lane(&id.parse().unwrap());
        for id in [
            "deck1.jog",
            "deck1.pitch",
            "deck2.play",
            "mixer.crossfader",
            "general.quantize",
            "deck1.sample1",
        ] {
            assert_eq!(of(id), Lane::Instant, "{id}");
        }
        for id in [
            "deck1.hotcue3",
            "deck1.cue",
            "deck2.tap",
            "deck1.sample2.load",
            "library.load1",
            "general.record",
        ] {
            assert_eq!(of(id), Lane::Slow, "{id}");
        }
    }
}
