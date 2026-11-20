//! The switches the actions turn (a deck's sync or key lock, an effect slot, the talkover, the
//! quantize…): which one a target turns and how, and whether it is on, read once for its key's
//! light and for what its key turns it to.

use crate::view::Sees;
use prisme_control::{
    DeckAction, DeckSwitch, FxAction, GeneralAction, MixerAction, MixerSwitch, Target, Turn,
};

/// A switch the actions turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switch {
    Deck(usize, DeckSwitch),
    /// A slot of a deck's effect chain: deck, slot.
    Fx(usize, usize),
    Mixer(MixerSwitch),
    /// A live source heard in the master.
    LiveOn(Live),
    /// A live source heard in the headphones.
    LiveCue(Live),
    Quantize,
}

/// A live source of the software mixer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Live {
    Mic,
    Aux,
}

impl Switch {
    /// The switch `target` turns, and how (the live sources' keys turn theirs over: they have
    /// no `.on` nor `.off`); none for a target that turns no switch.
    pub fn of(target: &Target) -> Option<(Switch, Turn)> {
        Some(match *target {
            Target::Deck(deck, DeckAction::Switch(switch, turn)) => (Switch::Deck(deck, switch), turn),
            Target::Deck(deck, DeckAction::Fx(slot, FxAction::Enabled(turn))) => {
                (Switch::Fx(deck, slot), turn)
            }
            Target::Mixer(MixerAction::Switch(switch, turn)) => (Switch::Mixer(switch), turn),
            Target::Mixer(MixerAction::MicOn) => (Switch::LiveOn(Live::Mic), Turn::Over),
            Target::Mixer(MixerAction::AuxOn) => (Switch::LiveOn(Live::Aux), Turn::Over),
            Target::Mixer(MixerAction::MicCue) => (Switch::LiveCue(Live::Mic), Turn::Over),
            Target::Mixer(MixerAction::AuxCue) => (Switch::LiveCue(Live::Aux), Turn::Over),
            Target::General(GeneralAction::Quantize(turn)) => (Switch::Quantize, turn),
            _ => return None,
        })
    }

    /// Whether it is on, as `sees` sees the application.
    pub fn is_on(self, sees: &impl Sees) -> bool {
        let live = |live: Live| {
            let mixer = sees.mixer();
            match live {
                Live::Mic => mixer.mic,
                Live::Aux => mixer.aux,
            }
        };
        match self {
            Switch::Deck(deck, switch) => match switch {
                DeckSwitch::Sync => sees.status(deck).sync.locked,
                DeckSwitch::Slip => sees.status(deck).slip_mode,
                DeckSwitch::Reverse => sees.status(deck).reverse,
                DeckSwitch::Repeat => sees.deck(deck).repeat,
                DeckSwitch::FaderStart => sees.deck(deck).fader_start,
                DeckSwitch::KeyLock => sees.deck(deck).key_lock,
                DeckSwitch::Headphones => sees.deck(deck).cue,
                DeckSwitch::Mute => sees.deck(deck).mute,
                DeckSwitch::Passthrough => sees.deck(deck).passthrough,
                DeckSwitch::Vinyl => sees.deck(deck).vinyl,
                DeckSwitch::JogScratch => sees.deck(deck).jog_scratch,
                DeckSwitch::FxChain => sees.deck(deck).fx.enabled,
            },
            Switch::Fx(deck, slot) => sees.deck(deck).fx.slots[slot].enabled,
            Switch::Mixer(switch) => {
                let mixer = sees.mixer();
                match switch {
                    MixerSwitch::HeadphonesSplit => mixer.split_cue,
                    MixerSwitch::Talkover => mixer.talkover,
                    MixerSwitch::CrossfaderReverse => mixer.crossfader_reverse,
                }
            }
            Switch::LiveOn(source) => live(source).on,
            Switch::LiveCue(source) => live(source).cue,
            Switch::Quantize => sees.quantize(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::fixtures::view;

    /// What the key of `id` turns its switch to, as `view` shows the application.
    fn turned(id: &str, view: &crate::view::View) -> Option<bool> {
        let (switch, turn) = Switch::of(&id.parse().unwrap())?;
        Some(turn.turns(switch.is_on(view)))
    }

    #[test]
    fn a_switch_turned_over_flips_and_one_turned_on_or_off_stays_so() {
        let mut v = view();
        v.decks[1].key_lock = true;
        assert_eq!(turned("deck2.keylock", &v), Some(false));
        assert_eq!(turned("deck2.keylock.on", &v), Some(true));
        assert_eq!(turned("deck1.keylock.off", &v), Some(false));
        v.mixer.mic.on = true;
        assert_eq!(turned("mixer.mic.on", &v), Some(false), "a toggle");
        assert_eq!(turned("general.quantize.on", &v), Some(true));
        v.status.decks[0].sync.locked = true;
        assert_eq!(turned("deck1.sync.on", &v), Some(true), "the engine's own");
        assert_eq!(turned("deck1.play", &v), None);
    }
}
