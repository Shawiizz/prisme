//! The actions Prisme lists, in the order the interface shows them, and their names.

use crate::label::Label;
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

    /// The group it is shown in.
    pub fn group(&self) -> Label {
        match self {
            Target::Deck(deck, _) => Label::new("Deck {deck}").with("deck", deck_letter(*deck)),
            Target::Mixer(_) => Label::new("Mixer"),
            Target::Library(_) => Label::new("Library"),
            Target::General(_) => Label::new("General"),
            Target::AutoDj(_) => Label::new("Auto DJ"),
            Target::Shift(_) | Target::Mode { .. } => Label::new("Controller"),
        }
    }

    pub fn label(&self) -> Label {
        match self {
            Target::Deck(_, action) => deck_label(*action),
            Target::Mixer(action) => mixer_label(*action),
            Target::Library(action) => library_label(*action),
            Target::General(GeneralAction::Quantize(turn)) => switched(Label::new("Quantize"), *turn),
            Target::General(action) => Label::new(match action {
                GeneralAction::Library => "The library over the whole window",
                GeneralAction::Record => "Record the mix",
                GeneralAction::TempoRange => "Range of every tempo fader: next",
                GeneralAction::TempoRangePrevious => "Range of every tempo fader: previous",
                GeneralAction::Help => "Keyboard shortcuts",
                GeneralAction::Palette => "Command palette",
                GeneralAction::Settings => "Settings",
                GeneralAction::Quantize(_) => unreachable!("labelled above"),
            }),
            Target::AutoDj(action) => Label::new(match action {
                AutoDjAction::Toggle => "Auto DJ on / off",
                AutoDjAction::Fade => "Mix into the next track now",
                AutoDjAction::Skip => "Skip the next track",
            }),
            Target::Shift(None) => Label::new("Shift (second use of the controls)"),
            Target::Shift(Some(name)) => Label::new("Shift: {name}").with("name", name.clone()),
            Target::Mode { group, mode } => Label::new("Mode {mode} of the {group} controls")
                .with("mode", mode.clone())
                .with("group", group.clone()),
        }
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

fn deck_letter(deck: usize) -> String {
    char::from(b'A' + deck as u8).to_string()
}

/// So many beats as a DJ writes them: 0.25 as "1/4", 2 as "2", -0.5 as "-1/2".
pub fn beats_text(beats: f64) -> String {
    let sign = if beats < 0.0 { "-" } else { "" };
    let size = beats.abs();
    if size >= 1.0 {
        format!("{sign}{size}")
    } else {
        format!("{sign}1/{}", (1.0 / size).round())
    }
}

fn switched(label: Label, turn: Turn) -> Label {
    match turn {
        Turn::Over => label,
        Turn::On => Label::new("{control}: on").with("control", label),
        Turn::Off => Label::new("{control}: off").with("control", label),
    }
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
    ]
};

fn deck_switch_label(switch: DeckSwitch) -> Label {
    Label::new(match switch {
        DeckSwitch::Sync => "Sync",
        DeckSwitch::Slip => "Slip",
        DeckSwitch::Reverse => "Reverse",
        DeckSwitch::Repeat => "Repeat the track",
        DeckSwitch::FaderStart => "Fader start",
        DeckSwitch::KeyLock => "Key lock",
        DeckSwitch::Headphones => "Headphones (pre-listen)",
        DeckSwitch::Mute => "Mute the channel",
        DeckSwitch::Passthrough => "Passthrough: the deck plays its input",
        DeckSwitch::Vinyl => "Vinyl control",
        DeckSwitch::JogScratch => "Jog wheel: a touch scratches, or only nudges",
        DeckSwitch::FxChain => "Effects (the whole chain)",
    })
}

fn effect_label(effect: Effect) -> Label {
    Label::new(match effect {
        Effect::Echo => "Echo",
        Effect::PingPong => "Ping-pong echo",
        Effect::Reverb => "Reverb",
        Effect::Flanger => "Flanger",
        Effect::Phaser => "Phaser",
        Effect::Filter => "Filter",
        Effect::Gate => "Gate",
        Effect::Bitcrusher => "Bitcrusher",
    })
}

fn fx_label(slot: usize, action: FxAction) -> Label {
    let key = match action {
        FxAction::Enabled(Turn::Over) => "Effect {n} on / off",
        FxAction::Enabled(Turn::On) => "Effect {n} on",
        FxAction::Enabled(Turn::Off) => "Effect {n} off",
        FxAction::Next => "Effect {n}: next effect",
        FxAction::Previous => "Effect {n}: previous effect",
        FxAction::Effect(effect) => {
            return Label::new("Effect {n}: {effect} on / off")
                .with("n", slot + 1)
                .with("effect", effect_label(effect))
        }
        FxAction::Mix => "Effect {n} level",
        FxAction::Amount => "Effect {n} amount",
        FxAction::Param2 => "Effect {n} second knob (tone, resonance… as the effect has)",
        FxAction::Beats => "Effect {n} period (a step halves or doubles it)",
        FxAction::Period => "Effect {n} period, along the knob",
        FxAction::Kind => "Effect {n}: the effect, along the knob",
        FxAction::Focus => "Effect {n} focused on the effect knobs / let go",
        FxAction::Held => "Effect {n} on while held",
    };
    Label::new(key).with("n", slot + 1)
}

fn sample_label(slot: usize, action: SampleAction) -> Label {
    Label::new(match action {
        SampleAction::Trigger => "Sample {n}",
        SampleAction::Stop => "Stop sample {n}",
        SampleAction::Eject => "Empty sample slot {n}",
        SampleAction::Load => "The selected track into sample slot {n}",
        SampleAction::Held => "Sample {n} while held",
        SampleAction::Smart => "Sample {n}: load the selected track, play, or stop",
        SampleAction::Velocity => "Sample {n} (as hard as the pad is hit)",
    })
    .with("n", slot + 1)
}

fn deck_label(action: DeckAction) -> Label {
    use DeckAction::*;
    let numbered = |key, n: usize| Label::new(key).with("n", n + 1);
    let key = match action {
        Switch(switch, turn) => return switched(deck_switch_label(switch), turn),
        HotCue(n) => return numbered("Hot cue {n}", n),
        HotCueDelete(n) => return numbered("Delete hot cue {n}", n),
        Sample(slot, action) => return sample_label(slot, action),
        Roll(n) => return Label::new("Roll {beats} while held").with("beats", beats_text(ROLL_BEATS[n])),
        Slice(n) => return numbered("Slice {n} while held", n),
        Cut(n) => {
            return Label::new("Fader cut on {beats} while held").with("beats", beats_text(CUT_BEATS[n]))
        }
        Fx(slot, action) => return fx_label(slot, action),
        JumpBeats(Beats(n)) => return Label::new("Jump {n} beats").with("n", n),
        LoopBeats(Beats(n)) => return Label::new("Loop {n} beats").with("n", n),
        RollBeats(Beats(n)) => return Label::new("Roll {n} beats while held").with("n", n),
        KeyTo(n) => return Label::new("Key moved {n} semitones").with("n", n),
        FxKnob(knob) => [
            "Effect knob 1: effect 1's amount, or the focused effect's",
            "Effect knob 2: effect 2's amount, or the focused effect's second knob",
            "Effect knob 3: effect 3's amount, or the focused effect's period",
        ][knob],
        Stem(part) => match part {
            StemPart::Vocals => "Vocals",
            StemPart::Instrumental => "Instrumental",
            StemPart::Drums => "Drums",
            StemPart::Bass => "Bass",
            StemPart::Melody => "Melody (the other instruments)",
        },
        StemLevel(part) => match part {
            StemPart::Vocals => "Vocals level",
            StemPart::Instrumental => "Instrumental level",
            StemPart::Drums => "Drums level",
            StemPart::Bass => "Bass level",
            StemPart::Melody => "Melody level",
        },
        Eq(band) => match band {
            Band::Low => "EQ low",
            Band::Mid => "EQ mid",
            Band::High => "EQ high",
        },
        EqKill(band) => match band {
            Band::Low => "Kill the bass while held",
            Band::Mid => "Kill the mids while held",
            Band::High => "Kill the highs while held",
        },
        EqKillToggle(band) => match band {
            Band::Low => "Kill the bass, or bring it back",
            Band::Mid => "Kill the mids, or bring them back",
            Band::High => "Kill the highs, or bring them back",
        },
        CrossfaderSide(side) => match side {
            Side::Left => "On the left of the crossfader",
            Side::Center => "Heard whatever the crossfader",
            Side::Right => "On the right of the crossfader",
        },
        Play => "Play / pause",
        Cue => "Cue (hold to preview)",
        Jog => "Jog wheel (turn)",
        JogTouch => "Jog wheel (touch: scratch)",
        JogSearch => "Jog wheel (search through the track)",
        Loop => "Loop 4 beats / exit",
        LoopIn => "Loop in",
        LoopOut => "Loop out",
        Reloop => "Reloop",
        LoopHalve => "Halve the loop",
        LoopDouble => "Double the loop",
        JumpBack => "Jump back by the size",
        JumpForward => "Jump ahead by the size",
        LoopSize => "Loop of the size / exit",
        SizeHalve => "Halve the size",
        SizeDouble => "Double the size",
        Size => "Size of the jumps and the loop (a step halves or doubles it)",
        BendDown => "Slow down while held",
        BendUp => "Speed up while held",
        TempoReset => "Tempo back to the track",
        TempoRange => "Tempo fader range: next",
        TempoRangePrevious => "Tempo fader range: previous",
        KeyUp => "Key up a semitone",
        KeyDown => "Key down a semitone",
        KeyReset => "Key back to the track",
        Censor => "Censor while held",
        Brake => "Brake (stopped: soft start)",
        Spinback => "Spinback while held",
        ReverseHeld => "Reverse while held",
        FxSuper => "Effect amounts together (super knob)",
        FxPart => "Effects on the next part",
        Pitch => "Tempo",
        Volume => "Volume",
        Trim => "Gain",
        Filter => "Filter",
        HeadphonesSolo => "This deck alone in the headphones",
        CuePlay => "To the cue and play",
        CueStop => "To the cue, stopped",
        CueGoto => "To the cue",
        CueSet => "The cue here",
        Start => "To the start, stopped",
        StartPlay => "To the start and play",
        StartKeep => "To the start",
        End => "To the end",
        Eject => "Eject the track",
        SyncOnce => "Match the tempo and beats once",
        SyncLeader => "Lead the other decks",
        SearchBack => "Run back through the track while held",
        SearchForward => "Run through the track while held",
        TempoUp => "Tempo up 0.1 %",
        TempoDown => "Tempo down 0.1 %",
        Tempo => "Tempo, 0.1 % a step",
        BendDownSmall => "Slow down a little while held",
        BendUpSmall => "Speed up a little while held",
        Position => "Place in the track (a touch strip)",
        KeySync => "Key matched to the other deck",
        Key => "Key, from 12 semitones down to 12 up",
        LoopClear => "Forget the loop",
        ReloopStop => "Back to the loop, stopped",
        LoopInGoto => "To the start of the loop",
        LoopOutGoto => "To the end of the loop",
        LoopMoveBack => "Loop a beat back",
        LoopMoveForward => "Loop a beat forward",
        LoopMove => "Loop moved, a beat a step",
        GridHere => "A beat of the grid here",
        GridEarlier => "Beat grid earlier",
        GridLater => "Beat grid later",
        GridFaster => "Beat grid faster",
        GridSlower => "Beat grid slower",
        GridMove => "Beat grid moved, a few milliseconds a step",
        Tap => "Tap the tempo",
        SlipHeld => "Slip while held",
        FilterToggle => "Filter heard or not",
        SamplerGain => "Sampler level",
        Beat => "Light: on the beat",
        Clip => "Light: the level past full scale",
        Ending => "Light: the end of the track near (soon: 10 s)",
        TempoZero => "Light: at the track's tempo",
        TempoSlower => "Light: slower than the deck it mixes with",
        TempoFaster => "Light: faster than the deck it mixes with",
        Phase => "Light: in phase with the leader",
        PhaseAhead => "Light: ahead of the leader",
        PhaseBehind => "Light: behind the leader",
        LoopAny => "Light: a loop playing",
    };
    Label::new(key)
}

fn mixer_label(action: MixerAction) -> Label {
    Label::new(match action {
        MixerAction::Switch(switch, turn) => {
            let label = Label::new(match switch {
                MixerSwitch::HeadphonesSplit => "Split headphones: cue left, master right",
                MixerSwitch::Talkover => "Talkover: the music ducks under the microphone",
                MixerSwitch::CrossfaderReverse => "Crossfader sides swapped",
            });
            return switched(label, turn);
        }
        MixerAction::Crossfader => "Crossfader",
        MixerAction::CueMix => "Headphones: cued decks or master",
        MixerAction::Balance => "Master balance",
        MixerAction::Booth => "Booth level",
        MixerAction::Mic => "Microphone level",
        MixerAction::MicOn => "Microphone on / off",
        MixerAction::MicCue => "Microphone in the headphones",
        MixerAction::Aux => "Aux line level",
        MixerAction::AuxOn => "Aux line on / off",
        MixerAction::AuxCue => "Aux line in the headphones",
        MixerAction::Headphones => "Headphones level",
        MixerAction::Main => "Master level",
        MixerAction::CrossfaderCurve => "Crossfader curve: smooth, then sharp",
        MixerAction::CrossfaderCurveNext => "The other crossfader curve",
        MixerAction::CrossfaderCurveSmooth => "Smooth crossfader curve",
        MixerAction::CrossfaderCurveSharp => "Sharp crossfader curve",
        MixerAction::SamplerGain => "Level of every deck's sampler",
        MixerAction::MainClip => "Light: the master past full scale",
    })
}

fn library_label(action: LibraryAction) -> Label {
    let deck = |key, deck: usize| Label::new(key).with("deck", deck_letter(deck));
    Label::new(match action {
        LibraryAction::Load(d) => return deck("Load on deck {deck}", d),
        LibraryAction::LoadPlay(d) => return deck("Load on deck {deck} and play", d),
        LibraryAction::Double(d) => return deck("Instant double on deck {deck}", d),
        LibraryAction::Browse => "Browse",
        LibraryAction::Previous => "Previous track",
        LibraryAction::Next => "Next track",
        LibraryAction::LoadFree => "Load on a free deck",
        LibraryAction::Preview => "Preview in the headphones / stop",
        LibraryAction::PreviewStop => "Stop the preview",
        LibraryAction::PreviewSeek => "Move through the preview",
        LibraryAction::Remove => "Remove from the crate or prepare list",
        LibraryAction::Prepare => "Add to the prepare list",
        LibraryAction::SelectAll => "Select all the tracks",
        LibraryAction::ClearSelection => "Clear the selection",
        LibraryAction::Search => "Search",
        LibraryAction::Focus => "Browse the lists or the tracks",
        LibraryAction::Lists => "Browse the lists (prepare, crates)",
        LibraryAction::ListOpen => "Open the list pointed at",
        LibraryAction::ListNext => "Open the next list",
        LibraryAction::ListPrevious => "Open the previous list",
        LibraryAction::Go => "Open the list, or load the track on a stopped deck",
        LibraryAction::Left => "To the lists",
        LibraryAction::Right => "To the tracks",
        LibraryAction::PagePrevious => "A page of tracks up",
        LibraryAction::PageNext => "A page of tracks down",
        LibraryAction::SortTitle => "Tracks by title",
        LibraryAction::SortArtist => "Tracks by artist",
        LibraryAction::SortBpm => "Tracks by tempo",
        LibraryAction::SortKey => "Tracks by key",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
