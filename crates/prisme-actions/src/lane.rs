//! Which actions keep their order among themselves: those of one lane. A lane is what an action
//! changes: a deck's track and where it plays, a deck's settings (its strip, its effects, its
//! tempo fader), a slot of its sampler, the mixer and what all decks share, the preview, the
//! recording, the library as it is browsed. An action is performed at once on the thread asking for it while its lane has
//! none waiting; one that reads or writes a file (a track or a sound opened, a recording ended)
//! is performed in turn by a worker of its lane, and the actions of that lane sent after it wait
//! behind it. No action waits for a file of another lane.

use prisme_control::{DeckAction, DeckSwitch, GeneralAction, LibraryAction, SampleAction, Target};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lane {
    /// The track of a deck, put there or taken off, and how and where it plays: what a load
    /// replaces.
    Track(usize),
    /// What a deck keeps from one track to the next: its strip, its effects, its tempo fader,
    /// its switches.
    Settings(usize),
    /// A slot of a deck's sampler.
    Sample(usize, usize),
    /// The mixer, and what every deck shares.
    Mixer,
    Preview,
    Recording,
    /// The list browsed, its rows picked, and the lists the library keeps.
    Library,
}

/// The lane of `target`. A load on the first deck without a track (`library.load`) is deck A's
/// until the deck it goes to is known.
pub fn lane(target: &Target) -> Lane {
    match *target {
        Target::Deck(deck, DeckAction::Sample(slot, _)) => Lane::Sample(deck, slot),
        Target::Deck(deck, action) if kept_by_the_deck(action) => Lane::Settings(deck),
        Target::Deck(deck, _) => Lane::Track(deck),
        Target::Library(
            LibraryAction::Load(deck) | LibraryAction::LoadPlay(deck) | LibraryAction::Double(deck),
        ) => Lane::Track(deck),
        Target::Library(LibraryAction::LoadFree) => Lane::Track(0),
        Target::Library(LibraryAction::Preview | LibraryAction::PreviewStop | LibraryAction::PreviewSeek) => {
            Lane::Preview
        }
        Target::Library(_) => Lane::Library,
        Target::General(GeneralAction::Record) => Lane::Recording,
        _ => Lane::Mixer,
    }
}

/// Whether `action` changes what a deck keeps from one track to the next.
fn kept_by_the_deck(action: DeckAction) -> bool {
    match action {
        DeckAction::Switch(switch, _) => {
            !matches!(switch, DeckSwitch::Sync | DeckSwitch::Slip | DeckSwitch::Reverse)
        }
        DeckAction::Pitch
        | DeckAction::Tempo
        | DeckAction::TempoUp
        | DeckAction::TempoDown
        | DeckAction::TempoReset
        | DeckAction::TempoRange
        | DeckAction::TempoRangePrevious
        | DeckAction::Volume
        | DeckAction::Trim
        | DeckAction::Filter
        | DeckAction::FilterToggle
        | DeckAction::Eq(_)
        | DeckAction::EqKill(_)
        | DeckAction::Fx(..)
        | DeckAction::FxKnob(_)
        | DeckAction::FxSuper
        | DeckAction::FxPart
        | DeckAction::SamplerGain
        | DeckAction::CrossfaderSide(_)
        | DeckAction::HeadphonesSolo
        | DeckAction::Size
        | DeckAction::SizeHalve
        | DeckAction::SizeDouble
        | DeckAction::SyncLeader => true,
        _ => false,
    }
}

/// Whether `target` reads or writes a file as it is performed: never on the thread asking for
/// it. A smart pad loads only into an empty slot, which the one sending it knows; a list is
/// opened, and the selection put in the prepare list or taken out of the list shown, in the
/// library's database or folders.
pub fn reads_files(target: &Target) -> bool {
    takes_selection(target)
        || matches!(
            target,
            Target::General(GeneralAction::Record)
                | Target::Library(
                    LibraryAction::ListOpen
                        | LibraryAction::ListNext
                        | LibraryAction::ListPrevious
                        | LibraryAction::Remove
                        | LibraryAction::Prepare
                )
        )
}

/// Whether `target` opens the track selected in the library: as it was when it was sent.
pub fn takes_selection(target: &Target) -> bool {
    matches!(
        target,
        Target::Deck(_, DeckAction::Sample(_, SampleAction::Load))
            | Target::Library(
                LibraryAction::Load(_)
                    | LibraryAction::LoadPlay(_)
                    | LibraryAction::LoadFree
                    | LibraryAction::Preview
            )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(id: &str) -> Lane {
        lane(&id.parse().unwrap())
    }

    #[test]
    fn a_load_and_what_plays_on_its_deck_share_a_lane_the_decks_settings_another() {
        for id in ["library.load2", "library.load2.play", "library.double2", "deck2.eject", "deck2.play"] {
            assert_eq!(of(id), Lane::Track(1), "{id}");
        }
        for id in ["deck1.hotcue3", "deck1.cue", "deck1.tap", "deck1.jog", "deck1.key.up", "deck1.sync"] {
            assert_eq!(of(id), Lane::Track(0), "{id}");
        }
        for id in [
            "deck1.pitch",
            "deck1.tempo.up",
            "deck1.volume",
            "deck1.fx2.mix",
            "deck1.keylock",
            "deck1.eq.low",
        ] {
            assert_eq!(of(id), Lane::Settings(0), "{id}");
        }
        assert_eq!(of("deck2.sample3.load"), Lane::Sample(1, 2));
        assert_eq!(of("deck2.sample3"), Lane::Sample(1, 2));
        assert_eq!(of("library.preview.stop"), Lane::Preview);
        assert_eq!(of("general.record"), Lane::Recording);
        assert_eq!(of("mixer.crossfader"), Lane::Mixer);
        for id in
            ["library.browse", "library.next", "library.list.open", "library.sort.bpm", "library.remove"]
        {
            assert_eq!(of(id), Lane::Library, "{id}");
        }
        assert_eq!(of("general.quantize"), Lane::Mixer);
    }

    #[test]
    fn what_opens_a_file_waits_its_turn_the_rest_is_done_at_once() {
        let reads = |id: &str| reads_files(&id.parse().unwrap());
        for id in [
            "library.load1",
            "library.load1.play",
            "library.load",
            "deck1.sample2.load",
            "library.preview",
            "general.record",
            "library.list.open",
            "library.list.next",
            "library.remove",
            "library.prepare",
        ] {
            assert!(reads(id), "{id}");
        }
        for id in [
            "deck1.hotcue3",
            "deck1.cue",
            "deck1.tap",
            "deck1.grid.here",
            "deck1.eject",
            "library.double2",
            "deck1.sample2.smart",
            "library.preview.stop",
            "library.next",
            "library.browse",
            "library.sort.title",
        ] {
            assert!(!reads(id), "{id}");
        }
        assert!(!takes_selection(&"general.record".parse().unwrap()), "a file of its own");
    }
}
