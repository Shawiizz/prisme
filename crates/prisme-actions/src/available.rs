//! Whether an action can do anything as its deck stands. One that cannot (a stem of a track not
//! separated where the deck plays, the key of a deck without a track, its place in none, its
//! tempo fader while a record or the sync holds the tempo, any action of a deck not in use, the
//! trim, EQ, volume and crossfader of the software mixer while the decks go to an external one)
//! changes nothing when its control moves, says nothing either, and its light and its ring are
//! off until it can again, on a controller as on the screen, where its control is disabled.

use crate::screen::tempo_held;
use crate::stems::{part_in, Separation};
use crate::view::Sees;
use prisme_control::{DeckAction, MixerAction, MixerSwitch, StemPart, Target};

/// What an action of a deck needs to do anything.
enum Needs {
    /// Nothing, or its own rules decide what it does without (a key moving the track, a cue,
    /// a loop: their lights show what they would do).
    Nothing,
    /// A track: its key, its grid.
    Track,
    /// A track the deck plays, of some length, to move in.
    Place,
    /// A track separated into stems at all.
    Stems,
    /// `part` of the track separated where the deck plays.
    Part(StemPart),
    /// The tempo set by the deck's fader: no record and no sync holding it.
    OwnTempo,
    /// The software mixer: the decks not sent to an external one.
    Mixer,
}

/// Whether `target` can do anything as `seen` stands (see the module); the actions of the
/// library and the application always can, those of the mixer but its crossfader too.
pub fn available(seen: &impl Sees, target: &Target) -> bool {
    let Target::Deck(deck, action) = *target else { return mixer_available(seen, target) };
    if deck >= seen.decks_in_use() {
        return false;
    }
    match needs(action) {
        Needs::Nothing => true,
        Needs::Track => seen.track(deck).is_some(),
        Needs::Place => {
            let status = seen.status(deck);
            status.loaded && status.duration_s > 0.0
        }
        Needs::Stems => seen.track(deck).is_some_and(|t| t.separation != Separation::None),
        Needs::Part(part) => {
            seen.track(deck).is_some_and(|t| t.separation.has(part))
                && part_in(seen.status(deck).stems_here, part)
        }
        Needs::OwnTempo => !tempo_held(seen, deck),
        Needs::Mixer => seen.mixer().internal,
    }
}

/// Whether `target`, none of a deck's, can do anything: the crossfader only while Prisme mixes.
fn mixer_available(seen: &impl Sees, target: &Target) -> bool {
    match target {
        Target::Mixer(
            MixerAction::Crossfader
            | MixerAction::CrossfaderCurve
            | MixerAction::CrossfaderCurveNext
            | MixerAction::CrossfaderCurveSmooth
            | MixerAction::CrossfaderCurveSharp
            | MixerAction::Switch(MixerSwitch::CrossfaderReverse, _),
        ) => seen.mixer().internal,
        _ => true,
    }
}

/// Every action decided, none by default: a new one must be.
fn needs(action: DeckAction) -> Needs {
    match action {
        DeckAction::Key
        | DeckAction::KeyUp
        | DeckAction::KeyDown
        | DeckAction::KeyReset
        | DeckAction::KeyTo(_)
        | DeckAction::KeySync
        | DeckAction::GridMove
        | DeckAction::GridHere
        | DeckAction::GridBar
        | DeckAction::GridPin
        | DeckAction::GridUnpin
        | DeckAction::GridEarlier
        | DeckAction::GridLater
        | DeckAction::GridFaster
        | DeckAction::GridSlower => Needs::Track,
        DeckAction::Position => Needs::Place,
        DeckAction::FxPart => Needs::Stems,
        DeckAction::Stem(part) | DeckAction::StemLevel(part) => Needs::Part(part),
        DeckAction::Pitch
        | DeckAction::Tempo
        | DeckAction::TempoUp
        | DeckAction::TempoDown
        | DeckAction::TempoReset => Needs::OwnTempo,
        DeckAction::Volume
        | DeckAction::Trim
        | DeckAction::Eq(_)
        | DeckAction::EqKill(_)
        | DeckAction::EqKillToggle(_)
        | DeckAction::CrossfaderSide(_) => Needs::Mixer,
        DeckAction::Play
        | DeckAction::Cue
        | DeckAction::CuePlay
        | DeckAction::CueStop
        | DeckAction::CueGoto
        | DeckAction::CueSet
        | DeckAction::Start
        | DeckAction::StartPlay
        | DeckAction::StartKeep
        | DeckAction::End
        | DeckAction::Eject
        | DeckAction::HotCue(_)
        | DeckAction::HotCueDelete(_)
        | DeckAction::Loop
        | DeckAction::LoopIn
        | DeckAction::LoopOut
        | DeckAction::Reloop
        | DeckAction::ReloopStop
        | DeckAction::LoopHalve
        | DeckAction::LoopDouble
        | DeckAction::LoopSize
        | DeckAction::LoopBeats(_)
        | DeckAction::LoopClear
        | DeckAction::LoopInGoto
        | DeckAction::LoopOutGoto
        | DeckAction::LoopMove
        | DeckAction::LoopMoveBack
        | DeckAction::LoopMoveForward
        | DeckAction::Roll(_)
        | DeckAction::RollBeats(_)
        | DeckAction::Slice(_)
        | DeckAction::Cut(_)
        | DeckAction::JumpBack
        | DeckAction::JumpForward
        | DeckAction::JumpBeats(_)
        | DeckAction::SearchBack
        | DeckAction::SearchForward
        | DeckAction::Jog
        | DeckAction::JogTouch
        | DeckAction::JogSearch
        | DeckAction::Censor
        | DeckAction::ReverseHeld
        | DeckAction::SlipHeld
        | DeckAction::Brake
        | DeckAction::Spinback
        | DeckAction::SyncOnce
        | DeckAction::SyncLeader
        | DeckAction::Switch(..)
        | DeckAction::TempoRange
        | DeckAction::TempoRangePrevious
        | DeckAction::BendDown
        | DeckAction::BendUp
        | DeckAction::PushSlower
        | DeckAction::PushFaster
        | DeckAction::BendDownSmall
        | DeckAction::BendUpSmall
        | DeckAction::Size
        | DeckAction::SizeHalve
        | DeckAction::SizeDouble
        | DeckAction::Tap
        | DeckAction::Filter
        | DeckAction::FilterToggle
        | DeckAction::HeadphonesSolo
        // a slot always holds one of the effects
        | DeckAction::Fx(..)
        | DeckAction::FxKnob(_)
        | DeckAction::FxSuper
        | DeckAction::Sample(..)
        | DeckAction::SamplerGain
        | DeckAction::Beat
        | DeckAction::Clip
        | DeckAction::Ending
        | DeckAction::TempoZero
        | DeckAction::TempoSlower
        | DeckAction::TempoFaster
        | DeckAction::Phase
        | DeckAction::PhaseAhead
        | DeckAction::PhaseBehind
        | DeckAction::LoopAny
        // the screen's pads, which any deck shows
        | DeckAction::Pads(_)
        | DeckAction::PadsPage => Needs::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::fixtures::{loaded, view};
    use crate::view::View;

    fn can(id: &str, view: &View) -> bool {
        available(view, &id.parse().unwrap_or_else(|e| panic!("{e}")))
    }

    /// Deck A holding a track separated into its vocals alone, where the deck plays.
    fn vocals_here() -> View {
        let mut v = loaded(view(), 0, 10.0, false);
        if let Some(track) = &mut v.decks[0].track {
            track.separation = Separation::Vocals;
        }
        v.status.decks[0].stems_here = 0b1001;
        v
    }

    #[test]
    fn a_stem_can_move_once_its_part_is_separated_where_the_deck_plays() {
        let stems = ["deck1.vocals", "deck1.vocals.level", "deck1.instrumental.level", "deck1.fx.part"];
        assert!(stems.iter().all(|id| !can(id, &view())), "no track");
        assert!(stems.iter().all(|id| !can(id, &loaded(view(), 0, 10.0, false))), "not separated");
        let mut coming = vocals_here();
        coming.status.decks[0].stems_here = 0;
        assert!(!can("deck1.vocals.level", &coming), "not separated there yet");
        assert!(can("deck1.fx.part", &coming), "separated elsewhere");
        let here = vocals_here();
        assert!(stems.iter().all(|id| can(id, &here)));
        assert!(!can("deck1.drums.level", &here), "a part of a full separation");
        assert!(!can("deck2.vocals", &here), "the other deck's track");
    }

    #[test]
    fn the_key_and_the_grid_of_a_deck_need_a_track_and_its_place_one_of_some_length() {
        let keys = ["deck1.key", "deck1.key.up", "deck1.key.0", "deck1.key.sync", "deck1.grid.move"];
        assert!(keys.iter().all(|id| !can(id, &view())));
        assert!(!can("deck1.position", &view()));
        let v = loaded(view(), 0, 10.0, false);
        assert!(keys.iter().all(|id| can(id, &v)));
        assert!(can("deck1.position", &v));
        let mut empty = v;
        empty.status.decks[0].duration_s = 0.0;
        assert!(!can("deck1.position", &empty), "of no length");
    }

    #[test]
    fn the_tempo_fader_cannot_move_while_a_record_or_the_sync_holds_the_tempo() {
        let tempo = ["deck1.pitch", "deck1.tempo", "deck1.tempo.up", "deck1.tempo.reset"];
        let mut v = loaded(view(), 0, 10.0, true);
        assert!(tempo.iter().all(|id| can(id, &v)));
        assert!(can("deck1.pitch", &view()), "without a track too");
        v.status.decks[0].sync.locked = true;
        assert!(tempo.iter().all(|id| !can(id, &v)), "the sync");
        assert!(can("deck1.bend.up", &v) && can("deck1.tempoRange", &v));
        v.status.decks[0].sync.locked = false;
        v.decks[0].vinyl = true;
        assert!(tempo.iter().all(|id| !can(id, &v)), "a record");
    }

    #[test]
    fn the_channel_the_effects_the_mixer_and_the_keys_moving_the_track_always_can() {
        let always = [
            "deck1.volume",
            "deck1.eq.low",
            "deck1.filter",
            "deck1.trim",
            "deck1.fx.mix",
            "deck1.fx2.period",
            "deck1.fx.knob1",
            "deck1.fx.super",
            "deck1.sampler.gain",
            "deck1.play",
            "deck1.loop.move",
            "deck1.jog",
            "mixer.crossfader",
            "mixer.mic",
            "library.browse",
        ];
        assert!(always.iter().all(|id| can(id, &view())));
    }

    #[test]
    fn with_an_external_mixer_the_filter_and_the_effects_can_its_own_trim_eq_volume_and_crossfader_not() {
        let mut v = view();
        v.mixer.internal = false;
        let its_own = ["deck1.volume", "deck1.trim", "deck1.eq.low", "deck1.eq.low.kill", "mixer.crossfader"];
        assert!(its_own.iter().all(|id| !can(id, &v)), "the external mixer's");
        let prisme = ["deck1.filter", "deck1.filter.toggle", "deck1.fx.mix", "mixer.main", "mixer.cueMix"];
        assert!(prisme.iter().all(|id| can(id, &v)));
    }

    #[test]
    fn nothing_of_a_deck_not_in_use_can() {
        let v = loaded(view(), 2, 10.0, false);
        assert!(!can("deck3.volume", &v));
        assert!(!can("deck3.key", &v));
        assert!(can("deck2.volume", &v));
    }
}
