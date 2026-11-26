//! A record holding its deck in place (ABS): where the needle is on the record is where the track
//! is, so nothing else may move the track, start it or stop it. What does is refused, with a word
//! told once for the messages of a gesture, and its key stays dark; the rest (the mixer, the
//! effects, the stems, a hot cue marked where the record is) goes on as on any deck.

use prisme_control::{DeckAction, DeckSwitch, MAX_DECKS};

/// How long after a refusal of a deck the next is not told (s), if none came between: a fader
/// moved, a wheel turned or a key held sends its refused messages over and over.
const QUIET_S: f64 = 1.0;

/// Whether a record holding its deck in place refuses `action` of the deck: it moves the track,
/// or starts or stops it. Hot cue `slot` is jumped to if it is set (`set`), else set where the
/// record is.
pub fn refuses(action: DeckAction, set: impl FnOnce(usize) -> bool) -> bool {
    match action {
        DeckAction::HotCue(slot) => set(slot),
        DeckAction::Play
        | DeckAction::Cue
        | DeckAction::CuePlay
        | DeckAction::CueStop
        | DeckAction::CueGoto
        | DeckAction::Start
        | DeckAction::StartPlay
        | DeckAction::StartKeep
        | DeckAction::End
        | DeckAction::Position
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
        | DeckAction::JumpBack
        | DeckAction::JumpForward
        | DeckAction::JumpBeats(_)
        | DeckAction::SearchBack
        | DeckAction::SearchForward
        | DeckAction::JogSearch
        | DeckAction::Censor
        | DeckAction::ReverseHeld
        | DeckAction::SlipHeld
        | DeckAction::Brake
        | DeckAction::Spinback
        // the record gives the tempo, and the place the beats are at
        | DeckAction::SyncOnce
        | DeckAction::Switch(DeckSwitch::Sync | DeckSwitch::Slip | DeckSwitch::Reverse, _) => true,
        // a jog wheel moves nothing under a record, in either mode
        DeckAction::Jog
        | DeckAction::JogTouch
        | DeckAction::CueSet
        | DeckAction::HotCueDelete(_)
        | DeckAction::Cut(_)
        | DeckAction::Eject
        | DeckAction::SyncLeader
        | DeckAction::Pitch
        | DeckAction::Tempo
        | DeckAction::TempoUp
        | DeckAction::TempoDown
        | DeckAction::TempoReset
        | DeckAction::TempoRange
        | DeckAction::TempoRangePrevious
        | DeckAction::BendDown
        | DeckAction::BendUp
        | DeckAction::BendDownSmall
        | DeckAction::BendUpSmall
        | DeckAction::Key
        | DeckAction::KeyUp
        | DeckAction::KeyDown
        | DeckAction::KeyReset
        | DeckAction::KeyTo(_)
        | DeckAction::KeySync
        | DeckAction::Size
        | DeckAction::SizeHalve
        | DeckAction::SizeDouble
        | DeckAction::GridHere
        | DeckAction::GridBar
        | DeckAction::GridPin
        | DeckAction::GridUnpin
        | DeckAction::GridEarlier
        | DeckAction::GridLater
        | DeckAction::GridFaster
        | DeckAction::GridSlower
        | DeckAction::GridMove
        | DeckAction::Tap
        | DeckAction::Volume
        | DeckAction::Trim
        | DeckAction::Eq(_)
        | DeckAction::EqKill(_)
        | DeckAction::EqKillToggle(_)
        | DeckAction::Filter
        | DeckAction::FilterToggle
        | DeckAction::HeadphonesSolo
        | DeckAction::CrossfaderSide(_)
        | DeckAction::Fx(..)
        | DeckAction::FxKnob(_)
        | DeckAction::FxSuper
        | DeckAction::FxPart
        | DeckAction::Stem(_)
        | DeckAction::StemLevel(_)
        | DeckAction::Sample(..)
        | DeckAction::SamplerGain
        | DeckAction::Switch(
            DeckSwitch::Repeat
            | DeckSwitch::FaderStart
            | DeckSwitch::KeyLock
            | DeckSwitch::Headphones
            | DeckSwitch::Mute
            | DeckSwitch::Passthrough
            | DeckSwitch::Vinyl
            | DeckSwitch::JogScratch
            | DeckSwitch::FxChain,
            _,
        )
        | DeckAction::Beat
        | DeckAction::Clip
        | DeckAction::Ending
        | DeckAction::TempoZero
        | DeckAction::TempoSlower
        | DeckAction::TempoFaster
        | DeckAction::Phase
        | DeckAction::PhaseAhead
        | DeckAction::PhaseBehind
        | DeckAction::LoopAny => false,
    }
}

/// When each deck last refused a controller an action, its record holding it in place: told
/// again once the deck refused none for a while.
#[derive(Debug, Default)]
pub struct Refusals([Option<f64>; MAX_DECKS]);

impl Refusals {
    /// `deck` refused an action at `now_s` (seconds, on any clock that goes on): whether the DJ
    /// is told.
    pub fn tells(&mut self, deck: usize, now_s: f64) -> bool {
        let quiet = self.0[deck].is_none_or(|at| now_s - at >= QUIET_S);
        self.0[deck] = Some(now_s);
        quiet
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prisme_control::{Target, Turn};

    fn refused(id: &str) -> bool {
        match id.parse::<Target>().unwrap_or_else(|e| panic!("{e}")) {
            Target::Deck(_, action) => refuses(action, |slot| slot == 0),
            target => panic!("{target} is no deck's"),
        }
    }

    #[test]
    fn what_moves_the_track_or_starts_or_stops_it_is_refused() {
        let families = [
            ["deck1.play", "deck1.cue", "deck1.cue.play", "deck1.cue.stop", "deck1.cue.goto"],
            ["deck1.start", "deck1.start.play", "deck1.start.keep", "deck1.end", "deck1.position"],
            ["deck1.loop", "deck1.loop.4", "deck1.loop.in", "deck1.loop.out", "deck1.loop.halve"],
            ["deck1.loop.double", "deck1.loop.size", "deck1.reloop", "deck1.reloop.stop", "deck1.loop.clear"],
            [
                "deck1.loop.in.goto",
                "deck1.loop.out.goto",
                "deck1.loop.move",
                "deck1.loop.move.back",
                "deck1.loop.move.forward",
            ],
            ["deck1.roll3", "deck1.roll.0.5", "deck1.slice2", "deck1.jump.back", "deck1.jump.-8"],
            [
                "deck1.jump.forward",
                "deck1.search.back",
                "deck1.search.forward",
                "deck1.jog.search",
                "deck1.censor",
            ],
            ["deck1.reverse.held", "deck1.slip.held", "deck1.brake", "deck1.spinback", "deck1.sync.once"],
            ["deck1.sync", "deck1.sync.on", "deck1.slip.off", "deck1.reverse", "deck1.hotcue1"],
        ];
        for id in families.iter().flatten() {
            assert!(refused(id), "{id}");
        }
    }

    #[test]
    fn the_mixer_the_effects_the_stems_the_tempo_and_a_cue_marked_go_on() {
        let allowed = [
            "deck1.hotcue2",
            "deck1.hotcue1.delete",
            "deck1.cue.set",
            "deck1.cut2",
            "deck1.volume",
            "deck1.eq.low",
            "deck1.filter",
            "deck1.trim",
            "deck1.fx2.echo",
            "deck1.fx",
            "deck1.fx.super",
            "deck1.vocals",
            "deck1.drums.level",
            "deck1.headphones",
            "deck1.keylock",
            "deck1.key.up",
            "deck1.pitch",
            "deck1.tempo.up",
            "deck1.bend.up",
            "deck1.sync.leader",
            "deck1.vinyl",
            "deck1.passthrough.on",
            "deck1.repeat",
            "deck1.eject",
            "deck1.sample3",
            "deck1.size.double",
            "deck1.grid.here",
            "deck1.tap",
            "deck1.jog",
            "deck1.jog.touch",
        ];
        for id in allowed {
            assert!(!refused(id), "{id}");
        }
        assert!(!refuses(DeckAction::Switch(DeckSwitch::Vinyl, Turn::Off), |_| true));
    }

    #[test]
    fn a_refusal_is_told_once_for_the_messages_of_a_gesture() {
        let mut refusals = Refusals::default();
        assert!(refusals.tells(0, 10.0));
        assert!(!refusals.tells(0, 10.4), "the same gesture");
        assert!(!refusals.tells(0, 11.2), "still going on");
        assert!(refusals.tells(1, 11.2), "another deck");
        assert!(refusals.tells(0, 12.5), "a gesture of its own");
    }
}
