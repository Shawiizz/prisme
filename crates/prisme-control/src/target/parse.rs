//! The actions read from their ids and written back as them: `deck1.hotcue3`,
//! `mixer.crossfader`, `mode.pads.roll`.

use super::*;

/// The amount a measured action's id ends with (`-8`, `0.5`, `+2`): a number as
/// `[+-]?\d+(\.\d+)?`.
fn signed_number(text: &str) -> Option<f64> {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, "0"));
    let numeric = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    (numeric(whole) && numeric(fraction)).then(|| text.parse().ok()).flatten()
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
        let amount = signed_number(amount)?;
        match family {
            "key" => (amount.fract() == 0.0).then_some(DeckAction::KeyTo(amount as i32)),
            "jump" => (amount != 0.0).then_some(DeckAction::JumpBeats(Beats(amount))),
            "loop" => (amount > 0.0).then_some(DeckAction::LoopBeats(Beats(amount))),
            "roll" => (amount > 0.0).then_some(DeckAction::RollBeats(Beats(amount))),
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
        // the first slot is `fx`, the others `fx2`, `fx3`…
        let slot = match slot {
            "" => 0,
            slot => number(slot, FX_SLOTS).filter(|&slot| slot > 0)?,
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
        if let Some(view) = id.strip_prefix("pads.").and_then(PadView::named) {
            return Some(DeckAction::Pads(view));
        }
        if let Some(part) = StemPart::named(id) {
            return Some(DeckAction::Stem(part));
        }
        if let Some(part) = id.strip_suffix(".level").and_then(StemPart::named) {
            return Some(DeckAction::StemLevel(part));
        }
        if let Some(band) = id.strip_prefix("eq.") {
            if let Some(band) = band.strip_suffix(".kill.toggle") {
                return Band::named(band).map(DeckAction::EqKillToggle);
            }
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
            DeckAction::Pads(view) => write!(f, "pads.{}", view.id()),
            DeckAction::Eq(band) => write!(f, "eq.{}", band.id()),
            DeckAction::EqKill(band) => write!(f, "eq.{}.kill", band.id()),
            DeckAction::EqKillToggle(band) => write!(f, "eq.{}.kill.toggle", band.id()),
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
pub(super) fn numbered<'i>(id: &'i str, prefix: &str, count: usize) -> Option<(usize, Option<&'i str>)> {
    let rest = id.strip_prefix(prefix)?;
    match rest.split_once('.') {
        Some((n, after)) => Some((number(n, count)?, Some(after))),
        None => Some((number(rest, count)?, None)),
    }
}
