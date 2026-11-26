//! The actions Prisme lists, in the order the interface shows them.

use crate::schema::ListedTarget;
use crate::target::*;

/// The length of each roll, in beats: the pads' and the controllers'.
pub const ROLL_BEATS: [f64; ROLLS] = [0.0625, 0.125, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0];
/// The length of each fader cut, in beats.
pub const CUT_BEATS: [f64; CUTS] = [0.0625, 0.125, 0.25, 0.5];

impl Target {
    /// Every action listed, in the order shown: all but those named by what they measure
    /// (`deck1.loop.0.5`), a controller's named shifts and its modes.
    pub fn all() -> Vec<Target> {
        let mut all: Vec<Target> = (0..MAX_DECKS)
            .flat_map(|deck| deck_actions().into_iter().map(move |a| Target::Deck(deck, a)))
            .collect();
        all.extend(GLOBAL.iter().flat_map(|global| global.targets()));
        all
    }

    /// Whether it acts with `decks` decks in use: one of a deck shown, or of none.
    pub fn in_use(&self, decks: usize) -> bool {
        self.deck().is_none_or(|deck| deck < decks)
    }
}

/// Every action listed, as the profiles' schema names them: in English.
pub fn listed_targets() -> Vec<ListedTarget> {
    Target::all()
        .iter()
        .map(|target| ListedTarget {
            id: target.to_string(),
            group: target.group().english(),
            label: target.label().english(),
            kind: target.kind(),
        })
        .collect()
}

fn deck_actions() -> Vec<DeckAction> {
    use DeckAction::*;
    let switched = |switch| Turn::ALL.map(|turn| Switch(switch, turn));
    let mut all = vec![Play, Cue];
    all.extend(switched(DeckSwitch::Sync));
    all.extend((0..HOT_CUES).map(HotCue));
    all.extend((0..HOT_CUES).map(HotCueDelete));
    all.extend([
        Jog,
        JogTouch,
        JogSearch,
        Loop,
        LoopIn,
        LoopOut,
        Reloop,
        LoopHalve,
        LoopDouble,
        JumpBack,
        JumpForward,
        LoopSize,
        SizeHalve,
        SizeDouble,
        Size,
        BendDown,
        BendUp,
        TempoReset,
        TempoRange,
        TempoRangePrevious,
        KeyUp,
        KeyDown,
        KeyReset,
    ]);
    all.extend(switched(DeckSwitch::Slip));
    all.extend([Censor, Brake, Spinback]);
    all.extend(switched(DeckSwitch::Reverse));
    all.push(ReverseHeld);
    for switch in [DeckSwitch::Repeat, DeckSwitch::FaderStart, DeckSwitch::KeyLock] {
        all.extend(switched(switch));
    }
    all.extend(StemPart::NAMED.iter().map(|part| Stem(*part)));
    all.extend(StemPart::NAMED.iter().map(|part| StemLevel(*part)));
    all.extend((0..SAMPLE_SLOTS).map(|slot| Sample(slot, SampleAction::Trigger)));
    for slot in 0..FX_SLOTS {
        let mut actions: Vec<FxAction> = Turn::ALL.map(FxAction::Enabled).to_vec();
        actions.extend([FxAction::Next, FxAction::Previous]);
        actions.extend(Effect::NAMED.iter().map(|effect| FxAction::Effect(*effect)));
        actions.extend([
            FxAction::Mix,
            FxAction::Amount,
            FxAction::Param2,
            FxAction::Beats,
            FxAction::Period,
            FxAction::Kind,
            FxAction::Focus,
            FxAction::Held,
        ]);
        all.extend(actions.into_iter().map(|action| Fx(slot, action)));
    }
    all.extend(switched(DeckSwitch::FxChain));
    all.extend((0..FX_KNOBS).map(FxKnob));
    all.extend([FxSuper, FxPart]);
    all.extend((0..ROLLS).map(Roll));
    all.extend((0..SLICES).map(Slice));
    all.extend((0..CUTS).map(Cut));
    all.extend([Pitch, Volume, Trim]);
    all.extend(Band::NAMED.iter().map(|band| Eq(*band)));
    all.extend(Band::NAMED.iter().map(|band| EqKill(*band)));
    all.extend(Band::NAMED.iter().map(|band| EqKillToggle(*band)));
    all.push(Filter);
    all.extend(switched(DeckSwitch::Headphones));
    all.extend(switched(DeckSwitch::Mute));
    all.push(HeadphonesSolo);
    all.extend(switched(DeckSwitch::Passthrough));
    all.extend([
        CuePlay,
        CueStop,
        CueGoto,
        CueSet,
        Start,
        StartPlay,
        StartKeep,
        End,
        Eject,
        SyncOnce,
        SyncLeader,
        SearchBack,
        SearchForward,
        TempoUp,
        TempoDown,
        Tempo,
        BendDownSmall,
        BendUpSmall,
        Position,
        KeySync,
        Key,
        LoopClear,
        ReloopStop,
        LoopInGoto,
        LoopOutGoto,
        LoopMoveBack,
        LoopMoveForward,
        LoopMove,
        GridHere,
        GridBar,
        GridPin,
        GridUnpin,
        GridEarlier,
        GridLater,
        GridFaster,
        GridSlower,
        GridMove,
        Tap,
        SlipHeld,
        FilterToggle,
    ]);
    all.extend(Side::NAMED.iter().map(|side| CrossfaderSide(*side)));
    all.push(SamplerGain);
    for slot in 0..SAMPLE_SLOTS {
        let actions = [
            SampleAction::Stop,
            SampleAction::Eject,
            SampleAction::Load,
            SampleAction::Held,
            SampleAction::Smart,
        ];
        all.extend(actions.map(|action| Sample(slot, action)));
    }
    all.extend(switched(DeckSwitch::Vinyl));
    all.extend(switched(DeckSwitch::JogScratch));
    all.extend([
        Beat,
        Clip,
        Ending,
        TempoZero,
        TempoSlower,
        TempoFaster,
        Phase,
        PhaseAhead,
        PhaseBehind,
        LoopAny,
    ]);
    all
}

/// The actions of no deck, in the order shown: one each, or a switch turned each way, or one
/// for each deck.
enum Global {
    One(Target),
    Switch(fn(Turn) -> Target),
    PerDeck(fn(usize) -> Target),
}

impl Global {
    fn targets(&self) -> Vec<Target> {
        match self {
            Global::One(target) => vec![target.clone()],
            Global::Switch(turned) => Turn::ALL.map(turned).to_vec(),
            Global::PerDeck(of) => (0..MAX_DECKS).map(of).collect(),
        }
    }
}

const GLOBAL: &[Global] = {
    use Global::{One, PerDeck, Switch};
    use LibraryAction as L;
    use MixerAction as M;
    &[
        One(Target::Mixer(M::Crossfader)),
        One(Target::Mixer(M::CueMix)),
        Switch(|turn| Target::Mixer(M::Switch(MixerSwitch::HeadphonesSplit, turn))),
        One(Target::Mixer(M::Balance)),
        One(Target::Mixer(M::Booth)),
        One(Target::Mixer(M::Mic)),
        One(Target::Mixer(M::MicOn)),
        One(Target::Mixer(M::MicCue)),
        One(Target::Mixer(M::Aux)),
        One(Target::Mixer(M::AuxOn)),
        One(Target::Mixer(M::AuxCue)),
        Switch(|turn| Target::Mixer(M::Switch(MixerSwitch::Talkover, turn))),
        One(Target::Shift(None)),
        One(Target::Library(L::Browse)),
        One(Target::Library(L::Previous)),
        One(Target::Library(L::Next)),
        PerDeck(|deck| Target::Library(L::Load(deck))),
        One(Target::Library(L::LoadFree)),
        PerDeck(|deck| Target::Library(L::LoadPlay(deck))),
        One(Target::Library(L::Preview)),
        One(Target::Library(L::PreviewStop)),
        One(Target::Library(L::PreviewSeek)),
        PerDeck(|deck| Target::Library(L::Double(deck))),
        One(Target::Library(L::Remove)),
        One(Target::Library(L::Prepare)),
        One(Target::Library(L::SelectAll)),
        One(Target::Library(L::ClearSelection)),
        One(Target::Library(L::Search)),
        One(Target::Library(L::Focus)),
        One(Target::Library(L::Lists)),
        One(Target::Library(L::ListOpen)),
        One(Target::Library(L::ListNext)),
        One(Target::Library(L::ListPrevious)),
        One(Target::Library(L::Go)),
        One(Target::Library(L::Left)),
        One(Target::Library(L::Right)),
        One(Target::Library(L::PagePrevious)),
        One(Target::Library(L::PageNext)),
        One(Target::Library(L::SortTitle)),
        One(Target::Library(L::SortArtist)),
        One(Target::Library(L::SortBpm)),
        One(Target::Library(L::SortKey)),
        One(Target::General(GeneralAction::Library)),
        One(Target::Mixer(M::Headphones)),
        One(Target::Mixer(M::Main)),
        One(Target::Mixer(M::CrossfaderCurve)),
        One(Target::Mixer(M::CrossfaderCurveNext)),
        One(Target::Mixer(M::CrossfaderCurveSmooth)),
        One(Target::Mixer(M::CrossfaderCurveSharp)),
        Switch(|turn| Target::Mixer(M::Switch(MixerSwitch::CrossfaderReverse, turn))),
        One(Target::Mixer(M::SamplerGain)),
        One(Target::Mixer(M::MainClip)),
        One(Target::General(GeneralAction::Record)),
        One(Target::AutoDj(AutoDjAction::Toggle)),
        One(Target::AutoDj(AutoDjAction::Fade)),
        One(Target::AutoDj(AutoDjAction::Skip)),
        Switch(|turn| Target::General(GeneralAction::Quantize(turn))),
        One(Target::General(GeneralAction::TempoRange)),
        One(Target::General(GeneralAction::TempoRangePrevious)),
        One(Target::General(GeneralAction::Help)),
        One(Target::General(GeneralAction::Palette)),
        One(Target::General(GeneralAction::Settings)),
        One(Target::General(GeneralAction::FullScreen)),
        One(Target::General(GeneralAction::Close)),
    ]
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::beats_text;
    use std::collections::BTreeSet;

    const TARGETS: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/prisme-desktop/controllers/targets.json");
    const LABELS: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../frontend/src/app/core/i18n/actions.json");

    /// `value` as `path` holds it, indented by `indent`; `PRISME_WRITE_TARGETS=1` writes it.
    fn up_to_date(path: &str, value: &impl serde::Serialize, indent: &[u8]) {
        let mut text = Vec::new();
        let mut writer = serde_json::Serializer::with_formatter(
            &mut text,
            serde_json::ser::PrettyFormatter::with_indent(indent),
        );
        value.serialize(&mut writer).unwrap();
        let text = String::from_utf8(text).unwrap() + "\n";
        if std::env::var_os("PRISME_WRITE_TARGETS").is_some() {
            std::fs::write(path, &text).unwrap();
        }
        assert_eq!(std::fs::read_to_string(path).unwrap().replace("\r\n", "\n"), text, "{path}");
    }

    #[test]
    fn the_actions_listed_for_the_profiles_are_up_to_date() {
        up_to_date(TARGETS, &listed_targets(), b" ");
    }

    /// The texts of every label, for the interface's translations (as Prettier writes them).
    #[test]
    fn the_names_to_translate_are_up_to_date() {
        let mut keys = BTreeSet::new();
        let measured =
            ["deck1.jump.-8", "deck1.loop.0.5", "deck1.roll.0.5", "deck1.key.+2", "deck1.sample1.velocity"];
        let named =
            ["shift.left", "mode.pads.roll"].iter().chain(&measured).map(|id| id.parse::<Target>().unwrap());
        for target in Target::all().into_iter().chain(named) {
            target.group().keys(&mut keys);
            target.label().keys(&mut keys);
        }
        up_to_date(LABELS, &keys, b"  ");
    }

    #[test]
    fn every_action_listed_reads_back_once() {
        let all = Target::all();
        let ids: BTreeSet<String> = all.iter().map(Target::to_string).collect();
        assert_eq!(ids.len(), all.len(), "each listed once");
        for target in &all {
            assert_eq!(target.to_string().parse::<Target>().as_ref(), Ok(target));
        }
    }

    /// The lists are written by hand: an action added to its enum is listed too.
    #[test]
    fn every_action_named_by_its_id_alone_is_listed() {
        use std::collections::HashSet;
        let listed: HashSet<Target> = Target::all().into_iter().collect();
        let deck = |action| Target::Deck(0, action);
        let samples = SampleAction::NAMED.iter().filter(|&&action| action != SampleAction::Velocity);
        let named = DeckAction::NAMED
            .iter()
            .map(|&action| deck(action))
            .chain(DeckSwitch::NAMED.iter().map(|&switch| deck(DeckAction::Switch(switch, Turn::Over))))
            .chain(FxAction::NAMED.iter().map(|&action| deck(DeckAction::Fx(0, action))))
            .chain(samples.map(|&action| deck(DeckAction::Sample(0, action))))
            .chain(MixerAction::NAMED.iter().map(|&action| Target::Mixer(action)))
            .chain(MixerSwitch::NAMED.iter().map(|&s| Target::Mixer(MixerAction::Switch(s, Turn::Over))))
            .chain(LibraryAction::NAMED.iter().map(|&action| Target::Library(action)))
            .chain(GeneralAction::NAMED.iter().map(|&action| Target::General(action)))
            .chain(AutoDjAction::NAMED.iter().map(|&action| Target::AutoDj(action)));
        let missing: Vec<String> = named.filter(|t| !listed.contains(t)).map(|t| t.to_string()).collect();
        assert!(missing.is_empty(), "not listed: {missing:?}");
    }

    #[test]
    fn measured_actions_say_their_number() {
        let label = |id: &str| id.parse::<Target>().unwrap().label().english();
        assert_eq!(label("deck2.jump.-8"), "Jump -8 beats");
        assert_eq!(label("deck1.loop.0.5"), "Loop 0.5 beats");
        assert_eq!(label("deck1.key.+2"), "Key moved 2 semitones");
        assert_eq!(label("deck2.sample3.velocity"), "Sample 3 (as hard as the pad is hit)");
        assert_eq!(label("deck1.cut1"), "Fader cut on 1/16 while held");
        assert_eq!([beats_text(-32.0), beats_text(-0.5), beats_text(0.0625)], ["-32", "-1/2", "1/16"]);
        assert_eq!("deck2.play".parse::<Target>().unwrap().group().english(), "Deck B");
    }

    #[test]
    fn the_actions_of_a_deck_not_shown_are_not_in_use() {
        let in_use = |id: &str| id.parse::<Target>().unwrap().in_use(2);
        assert!(in_use("deck2.play") && !in_use("deck3.play"));
        assert!(in_use("library.load2") && !in_use("library.double4"));
        assert!(in_use("mixer.crossfader"));
    }
}
