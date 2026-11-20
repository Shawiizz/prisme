//! The switches the actions turn (a deck's sync or key lock, an effect slot, the talkover, the
//! quantize…): whether each is on, read once for its key's light and for what its key turns it
//! to.

use crate::view::{MixerView, Sees};
use prisme_control::{
    DeckAction, DeckSwitch, FxAction, GeneralAction, MixerAction, MixerSwitch, Target, Turn,
};

/// Whether the switch `target` turns is on, as `sees` sees the application; none for a target
/// that turns no switch.
pub fn is_on(target: &Target, sees: &impl Sees) -> Option<bool> {
    Some(match *target {
        Target::Deck(deck, DeckAction::Switch(switch, _)) => {
            let settings = sees.deck(deck);
            match switch {
                DeckSwitch::Sync => sees.status(deck).sync.locked,
                DeckSwitch::Slip => sees.status(deck).slip_mode,
                DeckSwitch::Reverse => sees.status(deck).reverse,
                DeckSwitch::Repeat => settings.repeat,
                DeckSwitch::FaderStart => settings.fader_start,
                DeckSwitch::KeyLock => settings.key_lock,
                DeckSwitch::Headphones => settings.cue,
                DeckSwitch::Mute => settings.mute,
                DeckSwitch::Passthrough => settings.passthrough,
                DeckSwitch::Vinyl => settings.vinyl,
                DeckSwitch::JogScratch => settings.jog_scratch,
                DeckSwitch::FxChain => settings.fx.enabled,
            }
        }
        Target::Deck(deck, DeckAction::Fx(slot, FxAction::Enabled(_))) => {
            sees.deck(deck).fx.slots[slot].enabled
        }
        Target::Mixer(action) => mixer_on(action, &sees.mixer())?,
        Target::General(GeneralAction::Quantize(_)) => sees.quantize(),
        _ => return None,
    })
}

fn mixer_on(action: MixerAction, mixer: &MixerView) -> Option<bool> {
    Some(match action {
        MixerAction::Switch(MixerSwitch::HeadphonesSplit, _) => mixer.split_cue,
        MixerAction::Switch(MixerSwitch::Talkover, _) => mixer.talkover,
        MixerAction::Switch(MixerSwitch::CrossfaderReverse, _) => mixer.crossfader_reverse,
        MixerAction::MicOn => mixer.mic.on,
        MixerAction::MicCue => mixer.mic.cue,
        MixerAction::AuxOn => mixer.aux.on,
        MixerAction::AuxCue => mixer.aux.cue,
        _ => return None,
    })
}

/// How the key of `target` turns its switch: on, off, or over (the live sources' keys, which
/// have no `.on` nor `.off`).
pub fn turn_of(target: &Target) -> Turn {
    match *target {
        Target::Deck(_, DeckAction::Switch(_, turn) | DeckAction::Fx(_, FxAction::Enabled(turn)))
        | Target::Mixer(MixerAction::Switch(_, turn))
        | Target::General(GeneralAction::Quantize(turn)) => turn,
        _ => Turn::Over,
    }
}

/// What the key of `target` turns its switch to, from where it stands as `sees` sees it.
pub fn turned(target: &Target, sees: &impl Sees) -> Option<bool> {
    is_on(target, sees).map(|on| turn_of(target).turns(on))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::fixtures::view;

    fn target(id: &str) -> Target {
        id.parse().unwrap()
    }

    #[test]
    fn a_switch_turned_over_flips_and_one_turned_on_or_off_stays_so() {
        let mut v = view();
        v.decks[1].key_lock = true;
        assert_eq!(turned(&target("deck2.keylock"), &v), Some(false));
        assert_eq!(turned(&target("deck2.keylock.on"), &v), Some(true));
        assert_eq!(turned(&target("deck1.keylock.off"), &v), Some(false));
        v.mixer.mic.on = true;
        assert_eq!(turned(&target("mixer.mic.on"), &v), Some(false), "a toggle");
        assert_eq!(turned(&target("general.quantize.on"), &v), Some(true));
        v.status.decks[0].sync.locked = true;
        assert_eq!(is_on(&target("deck1.sync"), &v), Some(true), "the engine's own");
        assert_eq!(is_on(&target("deck1.play"), &v), None);
    }
}
