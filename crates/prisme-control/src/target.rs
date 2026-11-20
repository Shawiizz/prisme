//! The actions a control drives, by their typed form: what a profile's id (`deck1.play`,
//! `mixer.crossfader`, `deck2.loop.0.5`, `mode.pads.roll`) names, read once.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

pub const MAX_DECKS: usize = 4;
pub const HOT_CUES: usize = 16;
pub const SAMPLE_SLOTS: usize = 16;
pub const FX_SLOTS: usize = 3;
/// The knobs of a deck's effect chain.
pub const FX_KNOBS: usize = 3;
pub const ROLLS: usize = 8;
pub const SLICES: usize = 8;
pub const CUTS: usize = 4;

/// How a control drives an action: a key (pressed and let go, held or not), a fader over its
/// travel, an encoder by its steps, or a light that only shows a state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Button,
    Fader,
    Encoder,
    Light,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Kind::Button => "button",
            Kind::Fader => "fader",
            Kind::Encoder => "encoder",
            Kind::Light => "light",
        })
    }
}

/// Where an action is performed: by the application's core, straight from the controller, or
/// by the UI the controller tells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Rust,
    Ui,
}

/// Declares an enum whose unit variants are each named by an id and of a kind, the variants
/// that carry a value beside them.
macro_rules! actions {
    (
        $(#[$attr:meta])*
        $name:ident {
            $($kind:ident: $($variant:ident = $id:literal),+;)+
        }
        $({ $($(#[$extra_attr:meta])* $extra:ident($($field:ty),+),)+ })?
    ) => {
        $(#[$attr])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($($variant,)+)+
            $($($(#[$extra_attr])* $extra($($field),+),)+)?
        }

        impl $name {
            /// Every one named by its id alone, in the order declared.
            pub const NAMED: &[Self] = &[$($(Self::$variant,)+)+];

            /// Its id and its kind, if it is named by its id alone.
            #[allow(unreachable_patterns)]
            fn name(self) -> Option<(&'static str, Kind)> {
                match self {
                    $($(Self::$variant => Some(($id, Kind::$kind)),)+)+
                    _ => None,
                }
            }

            fn named(id: &str) -> Option<Self> {
                match id {
                    $($($id => Some(Self::$variant),)+)+
                    _ => None,
                }
            }
        }
    };
}

/// A switch's action: turned over, or turned on or off whatever it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Turn {
    Over,
    On,
    Off,
}

impl Turn {
    #[cfg(test)]
    const ALL: [Turn; 3] = [Turn::Over, Turn::On, Turn::Off];

    /// Whether a switch standing `on` is on once turned.
    pub fn turns(self, on: bool) -> bool {
        match self {
            Turn::Over => !on,
            Turn::On => true,
            Turn::Off => false,
        }
    }

    fn suffix(self) -> &'static str {
        match self {
            Turn::Over => "",
            Turn::On => ".on",
            Turn::Off => ".off",
        }
    }

    /// The switch's id and how `id` turns it.
    fn of(id: &str) -> (&str, Turn) {
        if let Some(switch) = id.strip_suffix(".on") {
            (switch, Turn::On)
        } else if let Some(switch) = id.strip_suffix(".off") {
            (switch, Turn::Off)
        } else {
            (id, Turn::Over)
        }
    }
}

/// So many beats (a jump back below zero), as a profile writes them: `-8`, `0.5`, `+2`.
#[derive(Debug, Clone, Copy)]
pub struct Beats(pub f64);

impl Beats {
    /// A number as `[+-]?\d+(\.\d+)?`.
    fn read(text: &str) -> Option<f64> {
        let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
        let (whole, fraction) = digits.split_once('.').unwrap_or((digits, "0"));
        let numeric = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
        (numeric(whole) && numeric(fraction)).then(|| text.parse().ok()).flatten()
    }
}

/// Never NaN: read from digits.
impl PartialEq for Beats {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for Beats {}

impl Hash for Beats {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

actions! {
    /// What a deck's switch is.
    DeckSwitch {
        Button: Sync = "sync", Slip = "slip", Reverse = "reverse", Repeat = "repeat",
            FaderStart = "faderStart", KeyLock = "keylock", Headphones = "headphones", Mute = "mute",
            Passthrough = "passthrough", Vinyl = "vinyl", JogScratch = "jog.scratch",
            FxChain = "fx.chain";
    }
}

actions! {
    /// A part of a separated track: one stem, or the instrumental (all but the vocals).
    StemPart {
        Button: Vocals = "vocals", Instrumental = "instrumental", Drums = "drums", Bass = "bass",
            Melody = "melody";
    }
}

actions! {
    /// A band of a deck's EQ.
    Band {
        Button: Low = "low", Mid = "mid", High = "high";
    }
}

actions! {
    /// A side of the crossfader a deck plays on, or neither.
    Side {
        Button: Left = "left", Center = "center", Right = "right";
    }
}

actions! {
    /// An effect a slot of a deck's chain plays.
    Effect {
        Button: Echo = "echo", PingPong = "pingPong", Reverb = "reverb", Flanger = "flanger",
            Phaser = "phaser", Filter = "filter", Gate = "gate", Bitcrusher = "bitcrusher";
    }
}

actions! {
    /// What a slot of a deck's effect chain does.
    FxAction {
        Button: Next = "next", Previous = "previous", Focus = "focus", Held = "held";
        Fader: Mix = "mix", Amount = "amount", Param2 = "param2", Period = "period", Kind = "kind";
        Encoder: Beats = "beats";
    }
    {
        Enabled(Turn),
        /// The effect played on, or off if it plays already.
        Effect(Effect),
    }
}

actions! {
    /// What a deck's sample slot does.
    SampleAction {
        Button: Trigger = "", Stop = "stop", Eject = "eject", Load = "load", Held = "held",
            Smart = "smart", Velocity = "velocity";
    }
}

actions! {
    /// What a deck does.
    DeckAction {
        Button: Play = "play", Cue = "cue", Loop = "loop", LoopIn = "loop.in", LoopOut = "loop.out",
            Reloop = "reloop", LoopHalve = "loop.halve", LoopDouble = "loop.double",
            JumpBack = "jump.back", JumpForward = "jump.forward", LoopSize = "loop.size",
            SizeHalve = "size.halve", SizeDouble = "size.double", BendDown = "bend.down",
            BendUp = "bend.up", TempoReset = "tempo.reset", TempoRange = "tempoRange",
            TempoRangePrevious = "tempoRange.previous", KeyUp = "key.up", KeyDown = "key.down",
            KeyReset = "key.reset", Censor = "censor", Brake = "brake", Spinback = "spinback",
            ReverseHeld = "reverse.held", HeadphonesSolo = "headphones.solo", CuePlay = "cue.play",
            CueStop = "cue.stop", CueGoto = "cue.goto", CueSet = "cue.set", Start = "start",
            StartPlay = "start.play", StartKeep = "start.keep", End = "end", Eject = "eject",
            SyncOnce = "sync.once", SyncLeader = "sync.leader", SearchBack = "search.back",
            SearchForward = "search.forward", TempoUp = "tempo.up", TempoDown = "tempo.down",
            BendDownSmall = "bend.down.small", BendUpSmall = "bend.up.small", KeySync = "key.sync",
            LoopClear = "loop.clear", ReloopStop = "reloop.stop", LoopInGoto = "loop.in.goto",
            LoopOutGoto = "loop.out.goto", LoopMoveBack = "loop.move.back",
            LoopMoveForward = "loop.move.forward", GridHere = "grid.here",
            GridEarlier = "grid.earlier", GridLater = "grid.later", GridFaster = "grid.faster",
            GridSlower = "grid.slower", Tap = "tap", SlipHeld = "slip.held",
            FilterToggle = "filter.toggle", FxPart = "fx.part", JogTouch = "jog.touch";
        Fader: Pitch = "pitch", Volume = "volume", Trim = "trim", Filter = "filter",
            Position = "position", Key = "key", SamplerGain = "sampler.gain", FxSuper = "fx.super";
        Encoder: Jog = "jog", JogSearch = "jog.search", Size = "size", Tempo = "tempo",
            LoopMove = "loop.move", GridMove = "grid.move";
        Light: Beat = "beat", Clip = "clip", Ending = "ending", TempoZero = "tempo.zero",
            TempoSlower = "tempo.slower", TempoFaster = "tempo.faster", Phase = "phase",
            PhaseAhead = "phase.ahead", PhaseBehind = "phase.behind", LoopAny = "loop.any";
    }
    {
        Switch(DeckSwitch, Turn),
        HotCue(usize),
        HotCueDelete(usize),
        Sample(usize, SampleAction),
        /// A roll of the length of its place among the rolls.
        Roll(usize),
        Slice(usize),
        Cut(usize),
        Fx(usize, FxAction),
        FxKnob(usize),
        Stem(StemPart),
        StemLevel(StemPart),
        Eq(Band),
        EqKill(Band),
        CrossfaderSide(Side),
        JumpBeats(Beats),
        LoopBeats(Beats),
        RollBeats(Beats),
        /// The key moved so many semitones.
        KeyTo(i32),
    }
}

actions! {
    /// What a mixer switch is.
    MixerSwitch {
        Button: HeadphonesSplit = "mixer.headphones.split", Talkover = "mixer.talkover",
            CrossfaderReverse = "mixer.crossfader.reverse";
    }
}

actions! {
    /// What the mixer does, the samplers' level and the master's clip light among it.
    MixerAction {
        Button: MicOn = "mixer.mic.on", MicCue = "mixer.mic.cue", AuxOn = "mixer.aux.on",
            AuxCue = "mixer.aux.cue", CrossfaderCurveNext = "mixer.crossfaderCurve.next";
        Fader: Crossfader = "mixer.crossfader", CueMix = "mixer.cueMix", Balance = "mixer.balance",
            Booth = "mixer.booth", Mic = "mixer.mic", Aux = "mixer.aux",
            Headphones = "mixer.headphones", Main = "mixer.main",
            CrossfaderCurve = "mixer.crossfaderCurve", SamplerGain = "sampler.gain";
        Light: MainClip = "main.clip";
    }
    {
        Switch(MixerSwitch, Turn),
    }
}

actions! {
    /// What the library does.
    LibraryAction {
        Button: Previous = "library.previous", Next = "library.next", LoadFree = "library.load",
            Preview = "library.preview", PreviewStop = "library.preview.stop",
            Remove = "library.remove", Prepare = "library.prepare",
            SelectAll = "library.selectAll", ClearSelection = "library.clearSelection",
            Search = "library.search", Focus = "library.focus", ListOpen = "library.list.open",
            ListNext = "library.list.next", ListPrevious = "library.list.previous",
            Go = "library.go", Left = "library.left", Right = "library.right",
            PagePrevious = "library.page.previous", PageNext = "library.page.next",
            SortTitle = "library.sort.title", SortArtist = "library.sort.artist",
            SortBpm = "library.sort.bpm", SortKey = "library.sort.key";
        Encoder: Browse = "library.browse", PreviewSeek = "library.preview.seek",
            Lists = "library.lists";
    }
    {
        /// The track selected loaded on a deck.
        Load(usize),
        LoadPlay(usize),
        /// A deck playing what the deck beside it plays.
        Double(usize),
    }
}

actions! {
    /// What the application does.
    GeneralAction {
        Button: Library = "general.library", Record = "general.record",
            TempoRange = "general.tempoRange", TempoRangePrevious = "general.tempoRange.previous",
            Help = "general.help", Palette = "general.palette", Settings = "general.settings";
    }
    {
        Quantize(Turn),
    }
}

actions! {
    /// What the Auto DJ does.
    AutoDjAction {
        Button: Toggle = "autodj.toggle", Fade = "autodj.fade", Skip = "autodj.skip";
    }
}

/// An action a control drives.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Target {
    /// Of a deck, by its place (0 for deck A).
    Deck(usize, DeckAction),
    Mixer(MixerAction),
    Library(LibraryAction),
    General(GeneralAction),
    AutoDj(AutoDjAction),
    /// A controller's shift, or one named (one per side, say), held: its controls' second
    /// layer.
    Shift(Option<String>),
    /// A group of a controller's controls put in a mode (`mode.<group>.<mode>`).
    Mode {
        group: String,
        mode: String,
    },
}

impl Target {
    pub fn kind(&self) -> Kind {
        match self {
            Target::Deck(_, action) => action.kind(),
            Target::Mixer(action) => action.name().map_or(Kind::Button, |(_, kind)| kind),
            Target::Library(action) => action.name().map_or(Kind::Button, |(_, kind)| kind),
            Target::General(_) | Target::AutoDj(_) | Target::Shift(_) | Target::Mode { .. } => Kind::Button,
        }
    }

    /// The deck it acts on, if it is a deck's.
    pub fn deck(&self) -> Option<usize> {
        match self {
            Target::Deck(deck, _) => Some(*deck),
            _ => None,
        }
    }

    pub fn owner(&self) -> Owner {
        match self {
            Target::Deck(
                _,
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
                | DeckAction::HotCue(_)
                | DeckAction::Roll(_)
                | DeckAction::RollBeats(_)
                | DeckAction::Slice(_)
                | DeckAction::Cut(_)
                | DeckAction::FxKnob(_)
                | DeckAction::FxPart
                | DeckAction::SearchBack
                | DeckAction::SearchForward
                | DeckAction::FilterToggle
                | DeckAction::Tap
                | DeckAction::KeySync
                | DeckAction::Stem(_)
                | DeckAction::Jog
                | DeckAction::JogTouch
                | DeckAction::JogSearch
                | DeckAction::Pitch
                | DeckAction::Brake
                | DeckAction::Spinback
                | DeckAction::Censor
                | DeckAction::ReverseHeld
                | DeckAction::SlipHeld
                | DeckAction::LoopIn
                | DeckAction::LoopOut
                | DeckAction::Reloop
                | DeckAction::LoopHalve
                | DeckAction::LoopDouble
                | DeckAction::LoopClear
                | DeckAction::ReloopStop
                | DeckAction::LoopInGoto
                | DeckAction::LoopOutGoto
                | DeckAction::LoopMoveBack
                | DeckAction::LoopMoveForward
                | DeckAction::LoopMove
                | DeckAction::JumpBeats(_)
                | DeckAction::LoopBeats(_)
                | DeckAction::BendDown
                | DeckAction::BendUp
                | DeckAction::BendDownSmall
                | DeckAction::BendUpSmall
                | DeckAction::SyncLeader
                | DeckAction::EqKill(_)
                | DeckAction::Fx(_, FxAction::Held)
                | DeckAction::Sample(
                    _,
                    SampleAction::Trigger | SampleAction::Stop | SampleAction::Held | SampleAction::Velocity,
                )
                | DeckAction::HotCueDelete(_)
                | DeckAction::GridHere
                | DeckAction::GridEarlier
                | DeckAction::GridLater
                | DeckAction::GridFaster
                | DeckAction::GridSlower
                | DeckAction::GridMove
                | DeckAction::Volume
                | DeckAction::Trim
                | DeckAction::Eq(_)
                | DeckAction::Filter
                | DeckAction::Position
                | DeckAction::Key
                | DeckAction::SamplerGain
                | DeckAction::StemLevel(_)
                | DeckAction::FxSuper
                | DeckAction::Fx(
                    _,
                    FxAction::Mix
                    | FxAction::Amount
                    | FxAction::Param2
                    | FxAction::Period
                    | FxAction::Kind
                    | FxAction::Beats
                    | FxAction::Next
                    | FxAction::Previous
                    | FxAction::Focus
                    | FxAction::Effect(_)
                    | FxAction::Enabled(_),
                )
                | DeckAction::Switch(..)
                | DeckAction::SyncOnce
                | DeckAction::HeadphonesSolo
                | DeckAction::CrossfaderSide(_)
                | DeckAction::TempoReset
                | DeckAction::TempoUp
                | DeckAction::TempoDown
                | DeckAction::Tempo
                | DeckAction::TempoRange
                | DeckAction::TempoRangePrevious
                | DeckAction::KeyUp
                | DeckAction::KeyDown
                | DeckAction::KeyReset
                | DeckAction::KeyTo(_)
                | DeckAction::JumpBack
                | DeckAction::JumpForward
                | DeckAction::Loop
                | DeckAction::LoopSize
                | DeckAction::SizeHalve
                | DeckAction::SizeDouble
                | DeckAction::Size,
            )
            | Target::Mixer(
                MixerAction::Crossfader
                | MixerAction::CueMix
                | MixerAction::Balance
                | MixerAction::Booth
                | MixerAction::Mic
                | MixerAction::Aux
                | MixerAction::Headphones
                | MixerAction::Main
                | MixerAction::CrossfaderCurve
                | MixerAction::SamplerGain
                | MixerAction::MicOn
                | MixerAction::MicCue
                | MixerAction::AuxOn
                | MixerAction::AuxCue
                | MixerAction::CrossfaderCurveNext
                | MixerAction::Switch(..),
            )
            | Target::General(
                GeneralAction::Quantize(_) | GeneralAction::TempoRange | GeneralAction::TempoRangePrevious,
            )
            | Target::AutoDj(_) => Owner::Rust,
            _ => Owner::Ui,
        }
    }

    /// Whether it takes several controls of a controller: a jog wheel sends its top and its
    /// edge apart.
    pub fn takes_several(&self) -> bool {
        matches!(self, Target::Deck(_, DeckAction::Jog | DeckAction::JogSearch))
    }

    fn of_deck(id: &str) -> Option<Self> {
        let rest = id.strip_prefix("deck")?;
        let (deck, action) = rest.split_once('.')?;
        Some(Target::Deck(number(deck, MAX_DECKS)?, action.parse().ok()?))
    }

    fn global(id: &str) -> Option<Self> {
        if let Some(action) = MixerAction::named(id) {
            return Some(Target::Mixer(action));
        }
        if let Some(action) = LibraryAction::named(id) {
            return Some(Target::Library(action));
        }
        if let Some(action) = GeneralAction::named(id) {
            return Some(Target::General(action));
        }
        if let Some(action) = AutoDjAction::named(id) {
            return Some(Target::AutoDj(action));
        }
        let (switch, turn) = Turn::of(id);
        if let Some(switch) = MixerSwitch::named(switch) {
            return Some(Target::Mixer(MixerAction::Switch(switch, turn)));
        }
        if switch == "general.quantize" {
            return Some(Target::General(GeneralAction::Quantize(turn)));
        }
        let load = |prefix| numbered(id, prefix, MAX_DECKS);
        let library = match (load("library.load"), load("library.double")) {
            (Some((deck, None)), _) => LibraryAction::Load(deck),
            (Some((deck, Some("play"))), _) => LibraryAction::LoadPlay(deck),
            (_, Some((deck, None))) => LibraryAction::Double(deck),
            _ => return None,
        };
        Some(Target::Library(library))
    }
}

/// An id Prisme does not know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unknown(pub String);

impl fmt::Display for Unknown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "no action is named {}", self.0)
    }
}

impl std::error::Error for Unknown {}

impl FromStr for Target {
    type Err = Unknown;

    fn from_str(id: &str) -> Result<Self, Unknown> {
        let target = if id.starts_with("deck") {
            Target::of_deck(id)
        } else if id == "shift" {
            Some(Target::Shift(None))
        } else if let Some(name) = id.strip_prefix("shift.") {
            (!name.is_empty()).then(|| Target::Shift(Some(name.to_owned())))
        } else if let Some(selected) = id.strip_prefix("mode.") {
            selected
                .split_once('.')
                .filter(|(group, mode)| !group.is_empty() && !mode.is_empty())
                .map(|(group, mode)| Target::Mode { group: group.to_owned(), mode: mode.to_owned() })
        } else {
            Target::global(id)
        };
        target.ok_or_else(|| Unknown(id.to_owned()))
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::Deck(deck, action) => write!(f, "deck{}.{action}", deck + 1),
            Target::Mixer(MixerAction::Switch(switch, turn)) => write!(f, "{}{}", switch.id(), turn.suffix()),
            Target::Mixer(action) => f.write_str(action.id()),
            Target::Library(LibraryAction::Load(deck)) => write!(f, "library.load{}", deck + 1),
            Target::Library(LibraryAction::LoadPlay(deck)) => write!(f, "library.load{}.play", deck + 1),
            Target::Library(LibraryAction::Double(deck)) => write!(f, "library.double{}", deck + 1),
            Target::Library(action) => f.write_str(action.id()),
            Target::General(GeneralAction::Quantize(turn)) => write!(f, "general.quantize{}", turn.suffix()),
            Target::General(action) => f.write_str(action.id()),
            Target::AutoDj(action) => f.write_str(action.id()),
            Target::Shift(None) => f.write_str("shift"),
            Target::Shift(Some(name)) => write!(f, "shift.{name}"),
            Target::Mode { group, mode } => write!(f, "mode.{group}.{mode}"),
        }
    }
}

/// The id of a variant named by it; the others are written by their enum's own.
trait Id: Copy {
    fn id(self) -> &'static str;
}

macro_rules! ids {
    ($($name:ident),+) => {
        $(impl Id for $name {
            fn id(self) -> &'static str {
                self.name().map_or("", |(id, _)| id)
            }
        })+
    };
}

ids!(
    DeckAction,
    DeckSwitch,
    StemPart,
    Band,
    Side,
    Effect,
    FxAction,
    SampleAction,
    MixerSwitch,
    MixerAction,
    LibraryAction,
    GeneralAction,
    AutoDjAction
);

impl DeckAction {
    pub fn kind(self) -> Kind {
        match self {
            DeckAction::Fx(_, action) => action.kind(),
            DeckAction::FxKnob(_) | DeckAction::StemLevel(_) | DeckAction::Eq(_) => Kind::Fader,
            action => action.name().map_or(Kind::Button, |(_, kind)| kind),
        }
    }

    /// One of the families named by what they measure.
    fn measured(id: &str) -> Option<Self> {
        let (family, amount) = id.split_once('.')?;
        let beats = Beats::read(amount)?;
        match family {
            "key" => (beats.fract() == 0.0).then_some(DeckAction::KeyTo(beats as i32)),
            "jump" => (beats != 0.0).then_some(DeckAction::JumpBeats(Beats(beats))),
            "loop" => (beats > 0.0).then_some(DeckAction::LoopBeats(Beats(beats))),
            "roll" => (beats > 0.0).then_some(DeckAction::RollBeats(Beats(beats))),
            _ => None,
        }
    }

    fn of_slot(id: &str) -> Option<Self> {
        if let Some((slot, rest)) = numbered(id, "hotcue", HOT_CUES) {
            return match rest {
                None => Some(DeckAction::HotCue(slot)),
                Some("delete") => Some(DeckAction::HotCueDelete(slot)),
                Some(_) => None,
            };
        }
        if let Some((slot, rest)) = numbered(id, "sample", SAMPLE_SLOTS) {
            let action = match rest {
                None => SampleAction::Trigger,
                Some(action) => SampleAction::named(action).filter(|a| *a != SampleAction::Trigger)?,
            };
            return Some(DeckAction::Sample(slot, action));
        }
        let alone =
            |prefix, count| numbered(id, prefix, count).filter(|(_, rest)| rest.is_none()).map(|(n, _)| n);
        if let Some(slot) = alone("roll", ROLLS) {
            return Some(DeckAction::Roll(slot));
        }
        if let Some(slot) = alone("slice", SLICES) {
            return Some(DeckAction::Slice(slot));
        }
        if let Some(slot) = alone("cut", CUTS) {
            return Some(DeckAction::Cut(slot));
        }
        if let Some(knob) = alone("fx.knob", FX_KNOBS) {
            return Some(DeckAction::FxKnob(knob));
        }
        let rest = id.strip_prefix("fx")?;
        let (slot, action) = match rest.split_once('.') {
            Some((slot, action)) => (slot, Some(action)),
            None => (rest, None),
        };
        let slot = match slot {
            "" => 0,
            "2" | "3" => number(slot, FX_SLOTS)?,
            _ => return None,
        };
        let action = match action {
            None => FxAction::Enabled(Turn::Over),
            Some("on") => FxAction::Enabled(Turn::On),
            Some("off") => FxAction::Enabled(Turn::Off),
            Some(action) => {
                FxAction::named(action).or_else(|| Effect::named(action).map(FxAction::Effect))?
            }
        };
        Some(DeckAction::Fx(slot, action))
    }

    fn of_part(id: &str) -> Option<Self> {
        if let Some(part) = StemPart::named(id) {
            return Some(DeckAction::Stem(part));
        }
        if let Some(part) = id.strip_suffix(".level").and_then(StemPart::named) {
            return Some(DeckAction::StemLevel(part));
        }
        if let Some(band) = id.strip_prefix("eq.") {
            return match band.strip_suffix(".kill") {
                Some(band) => Band::named(band).map(DeckAction::EqKill),
                None => Band::named(band).map(DeckAction::Eq),
            };
        }
        id.strip_prefix("crossfader.").and_then(Side::named).map(DeckAction::CrossfaderSide)
    }
}

impl FromStr for DeckAction {
    type Err = Unknown;

    fn from_str(id: &str) -> Result<Self, Unknown> {
        DeckAction::named(id)
            .or_else(|| {
                let (switch, turn) = Turn::of(id);
                DeckSwitch::named(switch).map(|switch| DeckAction::Switch(switch, turn))
            })
            .or_else(|| DeckAction::of_slot(id))
            .or_else(|| DeckAction::of_part(id))
            .or_else(|| DeckAction::measured(id))
            .ok_or_else(|| Unknown(id.to_owned()))
    }
}

impl fmt::Display for DeckAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let slot = |slot: &usize| slot + 1;
        match self {
            DeckAction::Switch(switch, turn) => write!(f, "{}{}", switch.id(), turn.suffix()),
            DeckAction::HotCue(n) => write!(f, "hotcue{}", slot(n)),
            DeckAction::HotCueDelete(n) => write!(f, "hotcue{}.delete", slot(n)),
            DeckAction::Sample(n, SampleAction::Trigger) => write!(f, "sample{}", slot(n)),
            DeckAction::Sample(n, action) => write!(f, "sample{}.{}", slot(n), action.id()),
            DeckAction::Roll(n) => write!(f, "roll{}", slot(n)),
            DeckAction::Slice(n) => write!(f, "slice{}", slot(n)),
            DeckAction::Cut(n) => write!(f, "cut{}", slot(n)),
            DeckAction::Fx(n, action) => {
                f.write_str("fx")?;
                if *n > 0 {
                    write!(f, "{}", slot(n))?;
                }
                match action {
                    FxAction::Enabled(turn) => f.write_str(turn.suffix()),
                    FxAction::Effect(effect) => write!(f, ".{}", effect.id()),
                    action => write!(f, ".{}", action.id()),
                }
            }
            DeckAction::FxKnob(n) => write!(f, "fx.knob{}", slot(n)),
            DeckAction::Stem(part) => f.write_str(part.id()),
            DeckAction::StemLevel(part) => write!(f, "{}.level", part.id()),
            DeckAction::Eq(band) => write!(f, "eq.{}", band.id()),
            DeckAction::EqKill(band) => write!(f, "eq.{}.kill", band.id()),
            DeckAction::CrossfaderSide(side) => write!(f, "crossfader.{}", side.id()),
            DeckAction::JumpBeats(Beats(beats)) => write!(f, "jump.{beats}"),
            DeckAction::LoopBeats(Beats(beats)) => write!(f, "loop.{beats}"),
            DeckAction::RollBeats(Beats(beats)) => write!(f, "roll.{beats}"),
            DeckAction::KeyTo(semitones) => write!(f, "key.{semitones}"),
            action => f.write_str(action.id()),
        }
    }
}

impl FxAction {
    fn kind(self) -> Kind {
        self.name().map_or(Kind::Button, |(_, kind)| kind)
    }
}

/// The place (from 0) of what `text` numbers from 1 to `count`, written without a leading zero.
pub(crate) fn number(text: &str, count: usize) -> Option<usize> {
    let numeric = !text.is_empty() && !text.starts_with('0') && text.bytes().all(|b| b.is_ascii_digit());
    let n: usize = text.parse().ok().filter(|_| numeric)?;
    (1..=count).contains(&n).then_some(n - 1)
}

/// The place of the one of `count` numbered right after `prefix` in `id`, and what follows
/// it past a dot.
fn numbered<'i>(id: &'i str, prefix: &str, count: usize) -> Option<(usize, Option<&'i str>)> {
    let rest = id.strip_prefix(prefix)?;
    match rest.split_once('.') {
        Some((n, after)) => Some((number(n, count)?, Some(after))),
        None => Some((number(rest, count)?, None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// The actions the application lists, as its page of controls writes them.
    fn listed() -> Vec<crate::ListedTarget> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/prisme-desktop/controllers/targets.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// Every action named by its id and its slots, one of each kind of switch: none measured.
    fn every_listed() -> Vec<Target> {
        let mut deck: Vec<DeckAction> = DeckAction::NAMED.to_vec();
        for switch in DeckSwitch::NAMED {
            deck.extend(Turn::ALL.map(|turn| DeckAction::Switch(*switch, turn)));
        }
        for n in 0..HOT_CUES {
            deck.extend([DeckAction::HotCue(n), DeckAction::HotCueDelete(n)]);
        }
        for n in 0..SAMPLE_SLOTS {
            let actions = SampleAction::NAMED.iter().filter(|a| **a != SampleAction::Velocity);
            deck.extend(actions.map(|action| DeckAction::Sample(n, *action)));
        }
        deck.extend((0..ROLLS).map(DeckAction::Roll));
        deck.extend((0..SLICES).map(DeckAction::Slice));
        deck.extend((0..CUTS).map(DeckAction::Cut));
        deck.extend((0..FX_KNOBS).map(DeckAction::FxKnob));
        for slot in 0..FX_SLOTS {
            let mut actions: Vec<FxAction> = FxAction::NAMED.to_vec();
            actions.extend(Turn::ALL.map(FxAction::Enabled));
            actions.extend(Effect::NAMED.iter().map(|e| FxAction::Effect(*e)));
            deck.extend(actions.into_iter().map(|action| DeckAction::Fx(slot, action)));
        }
        for part in StemPart::NAMED {
            deck.extend([DeckAction::Stem(*part), DeckAction::StemLevel(*part)]);
        }
        for band in Band::NAMED {
            deck.extend([DeckAction::Eq(*band), DeckAction::EqKill(*band)]);
        }
        deck.extend(Side::NAMED.iter().map(|s| DeckAction::CrossfaderSide(*s)));
        let mut all: Vec<Target> =
            (0..MAX_DECKS).flat_map(|d| deck.iter().map(move |a| Target::Deck(d, *a))).collect();
        all.extend(MixerAction::NAMED.iter().map(|a| Target::Mixer(*a)));
        for switch in MixerSwitch::NAMED {
            all.extend(Turn::ALL.map(|turn| Target::Mixer(MixerAction::Switch(*switch, turn))));
        }
        all.extend(LibraryAction::NAMED.iter().map(|a| Target::Library(*a)));
        for d in 0..MAX_DECKS {
            all.extend(
                [LibraryAction::Load(d), LibraryAction::LoadPlay(d), LibraryAction::Double(d)]
                    .map(Target::Library),
            );
        }
        all.extend(GeneralAction::NAMED.iter().map(|a| Target::General(*a)));
        all.extend(Turn::ALL.map(|turn| Target::General(GeneralAction::Quantize(turn))));
        all.extend(AutoDjAction::NAMED.iter().map(|a| Target::AutoDj(*a)));
        all.push(Target::Shift(None));
        all
    }

    #[test]
    fn every_action_listed_reads_and_writes_back_of_its_kind() {
        for listed in listed() {
            let target: Target = listed.id.parse().unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(target.to_string(), listed.id);
            assert_eq!(target.kind(), listed.kind, "{}", listed.id);
        }
    }

    #[test]
    fn the_actions_listed_are_those_prisme_knows() {
        let listed: BTreeSet<String> = listed().into_iter().map(|t| t.id).collect();
        let known: Vec<String> = every_listed().iter().map(Target::to_string).collect();
        assert_eq!(known.len(), listed.len(), "each written once");
        assert_eq!(known.into_iter().collect::<BTreeSet<_>>(), listed);
    }

    #[test]
    fn measured_actions_read_their_number() {
        let deck = |action| Some(Target::Deck(0, action));
        let read = |id: &str| id.parse::<Target>().ok();
        assert_eq!(read("deck1.jump.-8"), deck(DeckAction::JumpBeats(Beats(-8.0))));
        assert_eq!(read("deck1.loop.0.5"), deck(DeckAction::LoopBeats(Beats(0.5))));
        assert_eq!(read("deck1.roll.0.0625"), deck(DeckAction::RollBeats(Beats(0.0625))));
        assert_eq!(read("deck1.key.+2"), deck(DeckAction::KeyTo(2)));
        assert_eq!(read("deck1.key.0"), deck(DeckAction::KeyTo(0)));
        assert_eq!(
            read("deck4.sample16.velocity"),
            Some(Target::Deck(3, DeckAction::Sample(15, SampleAction::Velocity)))
        );
        assert_eq!(read("deck1.loop.0.6667").map(|t| t.to_string()).as_deref(), Some("deck1.loop.0.6667"));
        assert_eq!(
            read("deck2.jump.-0.03125").map(|t| t.to_string()).as_deref(),
            Some("deck2.jump.-0.03125")
        );
    }

    #[test]
    fn what_the_ui_refuses_is_unknown() {
        for id in [
            "deck1.jump.0",
            "deck1.loop.-1",
            "deck1.loop.0",
            "deck1.roll.-0.5",
            "deck1.key.1.5",
            "deck1.jump.1e3",
            "deck1.jump.inf",
            "deck1.loop..5",
            "deck1.loop.5.",
            "deck5.play",
            "deck0.play",
            "deck9.jog",
            "deck01.play",
            "deck1.sample17.velocity",
            "deck1.sample0",
            "deck1.sample1.",
            "deck1.hotcue17",
            "deck1.hotcue01",
            "deck1.roll9",
            "deck1.cut5",
            "deck1.fx1",
            "deck1.fx4.mix",
            "deck1.fx.chain.mix",
            "deck1.fx.knob4",
            "deck1.eq.top",
            "deck1.play.on",
            "library.load5",
            "library.double1.play",
            "mixer.mic.off",
            "general.palette.on",
            "shift.",
            "mode.pads",
            "mode..roll",
            "",
            "deck1.",
        ] {
            assert_eq!(id.parse::<Target>(), Err(Unknown(id.to_owned())), "{id}");
        }
    }

    #[test]
    fn shifts_and_modes_are_actions_of_their_controller() {
        let read = |id: &str| id.parse::<Target>().unwrap();
        assert_eq!(read("shift.left"), Target::Shift(Some("left".into())));
        let mode = read("mode.pads.hotcue");
        assert_eq!(mode, Target::Mode { group: "pads".into(), mode: "hotcue".into() });
        assert_eq!(mode.to_string(), "mode.pads.hotcue");
    }

    #[test]
    fn only_a_jog_wheel_takes_several_controls_of_a_controller() {
        let several: BTreeSet<String> = every_listed()
            .iter()
            .filter(|t| t.takes_several())
            .map(Target::to_string)
            .filter(|id| id.starts_with("deck2."))
            .collect();
        assert_eq!(several, BTreeSet::from(["deck2.jog".to_owned(), "deck2.jog.search".to_owned()]));
        assert_eq!("deck3.pitch".parse::<Target>().unwrap().deck(), Some(2));
        assert_eq!("mixer.crossfader".parse::<Target>().unwrap().deck(), None);
    }
}
