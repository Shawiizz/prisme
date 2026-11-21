//! The settings a controller moves, told the interface as they move, for it to show one its
//! screen leaves out: a phone shows the knob turned over what it shows. A setting moving is told
//! once in a while, not at each message of the knob; the first a fader sends only says where it
//! stands (a controller tells it as it is plugged in, or asked).

use crate::input::Input;
use prisme_control::{DeckAction, DeckSwitch, FxAction, Kind, MixerAction, Target};
use std::collections::{HashMap, HashSet};

/// How long a setting moving is told once (s).
const TOLD_EVERY_S: f64 = 0.25;

/// Whether `target` moves a setting a screen shows: a knob, a fader or a switch of a deck's
/// channel or of the mixer, a deck's tempo and key, its stems, its effects and its sampler's
/// level.
pub fn sets(target: &Target) -> bool {
    match target {
        Target::Deck(_, action) => matches!(
            action,
            DeckAction::Pitch
                | DeckAction::Tempo
                | DeckAction::TempoUp
                | DeckAction::TempoDown
                | DeckAction::TempoReset
                | DeckAction::Volume
                | DeckAction::Trim
                | DeckAction::Filter
                | DeckAction::FilterToggle
                | DeckAction::Eq(_)
                | DeckAction::EqKill(_)
                | DeckAction::EqKillToggle(_)
                | DeckAction::Key
                | DeckAction::KeyUp
                | DeckAction::KeyDown
                | DeckAction::KeyReset
                | DeckAction::KeyTo(_)
                | DeckAction::KeySync
                | DeckAction::HeadphonesSolo
                | DeckAction::Switch(DeckSwitch::KeyLock | DeckSwitch::Headphones | DeckSwitch::Mute, _)
                | DeckAction::Stem(_)
                | DeckAction::StemLevel(_)
                | DeckAction::SamplerGain
                | DeckAction::FxSuper
                | DeckAction::FxKnob(_)
                | DeckAction::Switch(DeckSwitch::FxChain, _)
                | DeckAction::Fx(
                    _,
                    FxAction::Mix
                        | FxAction::Amount
                        | FxAction::Param2
                        | FxAction::Period
                        | FxAction::Beats
                        | FxAction::Kind
                        | FxAction::Next
                        | FxAction::Previous
                        | FxAction::Effect(_)
                        | FxAction::Enabled(_)
                        | FxAction::Held
                )
        ),
        Target::Mixer(action) => matches!(
            action,
            MixerAction::Crossfader
                | MixerAction::CueMix
                | MixerAction::Headphones
                | MixerAction::Main
                | MixerAction::Balance
                | MixerAction::Booth
                | MixerAction::Mic
                | MixerAction::MicOn
                | MixerAction::Aux
                | MixerAction::AuxOn
                | MixerAction::SamplerGain
        ),
        _ => false,
    }
}

/// The settings told lately, and when, and the faders each controller was heard from.
#[derive(Debug, Default)]
pub struct Moves {
    told: Vec<(Target, f64)>,
    heard: HashMap<String, HashSet<Target>>,
}

impl Moves {
    /// The control of `device` driving the setting `target` (see [`sets`]) given `input` at
    /// `now_s` (seconds, on any clock that goes on): whether the interface is told. A key let go
    /// and a fader's first message move nothing; a setting told is told again once it has moved
    /// for a while, another at once.
    pub fn tells(&mut self, device: &str, target: &Target, input: Input, now_s: f64) -> bool {
        let moves = match input {
            Input::Value(_) if target.kind() == Kind::Fader => self.was_heard(device, target),
            Input::Value(value) => value > 0.0,
            Input::Steps { steps, .. } => steps != 0.0,
            Input::Set(_) => true,
        };
        if !moves {
            return false;
        }
        self.told.retain(|(_, at)| now_s - at < TOLD_EVERY_S);
        if self.told.iter().any(|(told, _)| told == target) {
            return false;
        }
        self.told.push((target.clone(), now_s));
        true
    }

    /// Whether the fader of `device` driving `target` was heard before, now that it is.
    fn was_heard(&mut self, device: &str, target: &Target) -> bool {
        if let Some(heard) = self.heard.get_mut(device) {
            return !heard.insert(target.clone());
        }
        self.heard.insert(device.to_owned(), HashSet::from([target.clone()]));
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(id: &str) -> Target {
        id.parse().unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn the_knobs_faders_and_switches_of_the_settings_are_told() {
        let settings = [
            "deck1.eq.low",
            "deck1.eq.mid.kill",
            "deck2.filter",
            "deck2.trim",
            "deck3.volume",
            "deck1.pitch",
            "deck1.tempo",
            "deck1.key.up",
            "deck1.key.+2",
            "deck1.keylock",
            "deck1.headphones.on",
            "deck1.vocals",
            "deck1.drums.level",
            "deck1.fx.knob2",
            "deck1.fx2.mix",
            "deck1.fx3.echo",
            "deck1.fx.on",
            "deck1.fx.chain",
            "deck1.sampler.gain",
            "mixer.crossfader",
            "mixer.headphones",
            "mixer.mic.on",
            "sampler.gain",
        ];
        let untold: Vec<_> = settings.iter().filter(|id| !sets(&target(id))).collect();
        assert!(untold.is_empty(), "{untold:?}");
    }

    #[test]
    fn what_plays_the_track_or_browses_is_not() {
        let others = [
            "deck1.play",
            "deck1.jog",
            "deck1.position",
            "deck1.hotcue1",
            "deck1.loop.4",
            "deck1.sync",
            "deck1.size",
            "deck1.fx.focus",
            "deck1.sample3",
            "library.browse",
            "general.library",
            "mixer.talkover",
            "shift",
        ];
        let told: Vec<_> = others.iter().filter(|id| sets(&target(id))).collect();
        assert!(told.is_empty(), "{told:?}");
    }

    #[test]
    fn a_setting_moving_is_told_once_in_a_while_another_at_once() {
        let mut moves = Moves::default();
        let (eq, filter) = (target("deck1.eq.low"), target("deck1.filter"));
        let mut tells = |target: &Target, at: f64| moves.tells("DDJ", target, Input::Value(0.4), at);
        assert!(!tells(&eq, 0.0), "where the knob stands");
        assert!(!tells(&filter, 0.0));
        let told: Vec<bool> = [0.1, 0.2, 0.3].iter().map(|&at| tells(&eq, at)).collect();
        assert_eq!(told, [true, false, false]);
        assert!(tells(&filter, 0.31), "another setting");
        assert!(!tells(&eq, 0.34));
        assert!(tells(&eq, 0.36), "moved a while");
        assert!(!tells(&filter, 0.4));
    }

    #[test]
    fn a_key_is_told_pressed_an_encoder_turned_and_a_fader_once_its_controller_was_heard() {
        let mut moves = Moves::default();
        let vocals = target("deck1.vocals");
        assert!(!moves.tells("DDJ", &vocals, Input::Value(0.0), 0.0), "let go");
        assert!(moves.tells("DDJ", &vocals, Input::Value(1.0), 0.0));
        let tempo = target("deck1.tempo");
        assert!(!moves.tells("DDJ", &tempo, Input::Steps { steps: 0.0, step: None }, 0.0));
        assert!(moves.tells("DDJ", &tempo, Input::Steps { steps: -1.0, step: None }, 0.0));
        let volume = target("deck1.volume");
        assert!(!moves.tells("DDJ", &volume, Input::Value(0.0), 1.0));
        assert!(moves.tells("DDJ", &volume, Input::Value(0.0), 1.1), "at its bottom");
        assert!(!moves.tells("S4", &volume, Input::Value(0.5), 2.0), "another controller's first");
        assert!(moves.tells("DDJ", &volume, Input::Set(1.0), 2.5), "a key putting it there");
    }
}
