//! How a control's moves reach its action: a fader picks its setting up before moving it, an
//! encoder's steps below one add up, an encoder on a fader's action moves it from where it
//! stands. Two controllers on one action each pick it up on their own.

use crate::view::band_index;
use prisme_control::{
    DeckAction, FxAction, Kind, MixerAction, Pickup, StepCarry, Target, FX_SLOTS, MAX_DECKS,
};
use std::collections::HashMap;

/// How far a step of an encoder moves a fader's action when its profile does not say.
const ENCODER_STEP: f32 = 1.0 / 64.0;

/// What a control gives its action.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    /// A key's value (down above 0, however softly) or a fader's position, 0 to 1.
    Value(f32),
    /// An encoder's steps, each moving a fader's action by `step` of its travel.
    Steps { steps: f32, step: Option<f32> },
    /// A position a key puts a fader's action at, whatever it stands at: no pickup.
    Set(f32),
}

impl Input {
    /// `value`, or so many `steps` if it is an encoder's.
    pub fn of(value: f32, steps: bool, step: Option<f32>) -> Self {
        if steps {
            Input::Steps { steps: value, step }
        } else {
            Input::Value(value)
        }
    }

    /// How much it moves: a value, or steps.
    pub fn amount(self) -> f32 {
        match self {
            Input::Value(value) | Input::Set(value) => value,
            Input::Steps { steps, .. } => steps,
        }
    }
}

/// What an action is given once its control is read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Given {
    /// A key's value, or a fader's new position.
    Value(f32),
    /// Whole steps of an encoder.
    Steps(i32),
}

impl Given {
    /// A key's: down, however softly a pad was hit.
    pub fn pressed(self) -> bool {
        matches!(self, Given::Value(value) if value > 0.0)
    }
}

/// What is kept of a controller's control of an action between its moves.
#[derive(Debug, Default)]
struct ControlMemory {
    pickup: Pickup,
    /// What is left of the steps short of a whole one.
    steps: StepCarry,
}

/// The controls of the controllers, by controller and action.
#[derive(Debug, Default)]
pub struct Inputs {
    devices: HashMap<String, HashMap<Target, ControlMemory>>,
}

impl Inputs {
    /// What `input` of a control of `device` gives `target`, whose setting stands at
    /// `position` if it is a fader's (0 to 1); `None` while it gives nothing: a fader that has
    /// not picked its setting up, steps short of a whole one, an encoder on a key.
    pub fn resolve(
        &mut self,
        device: &str,
        target: &Target,
        input: Input,
        position: Option<f32>,
    ) -> Option<Given> {
        match input {
            Input::Steps { steps, step } if target.kind() != Kind::Encoder => {
                position.map(|at| Given::Value((at + steps * step.unwrap_or(ENCODER_STEP)).clamp(0.0, 1.0)))
            }
            Input::Steps { steps, .. } => {
                let whole = self.memory(device, target).steps.whole(steps);
                (whole != 0).then_some(Given::Steps(whole))
            }
            Input::Value(value) => match position {
                Some(at) => {
                    self.memory(device, target).pickup.takes(value, at).then_some(Given::Value(value))
                }
                None => Some(Given::Value(value)),
            },
            Input::Set(value) => Some(Given::Value(value)),
        }
    }

    /// The name of `device` is copied once, on its first move, not on each.
    fn memory(&mut self, device: &str, target: &Target) -> &mut ControlMemory {
        if !self.devices.contains_key(device) {
            self.devices.insert(device.to_owned(), HashMap::new());
        }
        let held = self.devices.get_mut(device).expect("inserted above");
        held.entry(target.clone()).or_default()
    }
}

/// A fader over a few steps (the effects, the periods), each an equal part of its travel. Its
/// position is the one it was last given while that still gives the step in use: a
/// controller's fader moving within a step keeps it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Stepped {
    given: Option<f32>,
}

impl Stepped {
    /// The step of `count` at `position`.
    pub fn step_at(position: f32, count: usize) -> usize {
        ((position * count as f32).floor().max(0.0) as usize).min(count - 1)
    }

    /// Given `position`: the step of `count` it gives.
    pub fn give(&mut self, position: f32, count: usize) -> usize {
        self.given = Some(position);
        Self::step_at(position, count)
    }

    /// Where it stands with step `step` of `count` in use.
    pub fn position(&self, step: usize, count: usize) -> f32 {
        match self.given {
            Some(given) if Self::step_at(given, count) == step => given,
            _ => Self::centre(step, count),
        }
    }

    /// The middle of step `step` of `count`.
    pub fn centre(step: usize, count: usize) -> f32 {
        (step as f32 + 0.5) / count as f32
    }
}

/// A fader silent over the start of its travel (an EQ band, a live input), where its setting
/// reads as full left: its position is the one it was last given while the setting is still
/// the one that gave, so that a controller's fader moving there keeps it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SilentEnd {
    /// The position given, and the setting's position it gave.
    given: Option<(f32, f32)>,
}

impl SilentEnd {
    /// Given `position`, the setting then reading as `setting`.
    pub fn give(&mut self, position: f32, setting: f32) {
        self.given = Some((position, setting));
    }

    /// Where it stands, its setting reading as `setting`.
    pub fn position(&self, setting: f32) -> f32 {
        match self.given {
            Some((given, gave)) if gave == setting => given,
            _ => setting,
        }
    }
}

/// Where the faders over a few steps or silent at an end were last given, by action, whoever
/// moved them (see [`Stepped`], [`SilentEnd`]): every deck's EQ bands, effect periods and
/// effects and key, the live sources' levels, the crossfader's curve.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Faders {
    silent_ends: [SilentEnd; SILENT_ENDS],
    stepped: [Stepped; STEPPED],
}

/// The EQ bands of every deck, then the microphone's and the aux line's levels.
const SILENT_ENDS: usize = MAX_DECKS * 3 + 2;
/// The period then the effect of every deck's slots, every deck's key, then the crossfader's
/// curve.
const STEPPED: usize = MAX_DECKS * FX_SLOTS * 2 + MAX_DECKS + 1;
/// The keys a deck's key fader goes over: 12 semitones down to 12 up.
pub const KEY_STEPS: usize = 25;

impl Faders {
    /// The memory of `target`, if it is silent at an end.
    pub fn silent_end(&self, target: &Target) -> SilentEnd {
        silent_end(target).map_or(SilentEnd::default(), |i| self.silent_ends[i])
    }

    /// The memory of `target`, if it is over steps.
    pub fn stepped(&self, target: &Target) -> Stepped {
        stepped(target).map_or(Stepped::default(), |i| self.stepped[i])
    }

    /// `target`, silent at an end, given `position`, its setting then reading as `setting`.
    pub fn give_silent_end(&mut self, target: &Target, position: f32, setting: f32) {
        if let Some(i) = silent_end(target) {
            self.silent_ends[i].give(position, setting);
        }
    }

    /// `target`, over `count` steps, given `position`: the step it gives.
    pub fn give_stepped(&mut self, target: &Target, position: f32, count: usize) -> usize {
        match stepped(target) {
            Some(i) => self.stepped[i].give(position, count),
            None => Stepped::step_at(position, count),
        }
    }
}

fn silent_end(target: &Target) -> Option<usize> {
    match *target {
        Target::Deck(deck, DeckAction::Eq(band)) => Some(deck * 3 + band_index(band)),
        Target::Mixer(MixerAction::Mic) => Some(SILENT_ENDS - 2),
        Target::Mixer(MixerAction::Aux) => Some(SILENT_ENDS - 1),
        _ => None,
    }
}

fn stepped(target: &Target) -> Option<usize> {
    match *target {
        Target::Deck(deck, DeckAction::Fx(slot, FxAction::Period)) => Some((deck * FX_SLOTS + slot) * 2),
        Target::Deck(deck, DeckAction::Fx(slot, FxAction::Kind)) => Some((deck * FX_SLOTS + slot) * 2 + 1),
        Target::Deck(deck, DeckAction::Key) => Some(MAX_DECKS * FX_SLOTS * 2 + deck),
        Target::Mixer(MixerAction::CrossfaderCurve) => Some(STEPPED - 1),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scales::{eq_gain, eq_knob, live_gain, live_knob};

    fn target(id: &str) -> Target {
        id.parse().unwrap()
    }

    /// A 7-bit knob from full left to `to`.
    fn knob(to: f32) -> Vec<f32> {
        (0..=(to * 127.0).round() as u32).map(|i| i as f32 / 127.0).collect()
    }

    #[test]
    fn a_fader_picks_its_setting_up_on_each_controller_apart() {
        let mut inputs = Inputs::default();
        let volume = target("deck1.volume");
        let mut move_to = |device, value, at| inputs.resolve(device, &volume, Input::Value(value), Some(at));
        assert_eq!(move_to("DDJ", 0.5, 0.5), Some(Given::Value(0.5)));
        assert_eq!(move_to("Xone", 0.1, 0.5), None, "the other controller's fader is elsewhere");
        assert_eq!(move_to("DDJ", 0.6, 0.5), Some(Given::Value(0.6)));
        assert_eq!(move_to("Xone", 0.7, 0.6), Some(Given::Value(0.7)), "crossed it");
    }

    #[test]
    fn a_key_puts_a_fader_where_it_says_whatever_it_stands_at() {
        let mut inputs = Inputs::default();
        let crossfader = target("mixer.crossfader");
        assert_eq!(
            inputs.resolve("DDJ", &crossfader, Input::Value(0.5), Some(0.0)),
            None,
            "a fader picks up"
        );
        assert_eq!(inputs.resolve("DDJ", &crossfader, Input::Set(0.5), Some(0.0)), Some(Given::Value(0.5)));
    }

    #[test]
    fn a_key_and_a_fader_with_no_setting_take_their_value_as_it_comes() {
        let mut inputs = Inputs::default();
        assert_eq!(
            inputs.resolve("DDJ", &target("deck1.play"), Input::Value(0.3), None),
            Some(Given::Value(0.3))
        );
        let crossfader = target("mixer.crossfader");
        assert_eq!(inputs.resolve("DDJ", &crossfader, Input::Value(0.9), None), Some(Given::Value(0.9)));
    }

    #[test]
    fn an_encoder_adds_up_its_steps_below_one() {
        let mut inputs = Inputs::default();
        let browse = target("library.browse");
        let mut turn = |steps| inputs.resolve("DDJ", &browse, Input::Steps { steps, step: None }, None);
        assert_eq!(turn(0.4), None);
        assert_eq!(turn(0.4), None);
        assert_eq!(turn(0.4), Some(Given::Steps(1)));
        assert_eq!(turn(-2.5), Some(Given::Steps(-2)));
        assert_eq!(turn(-0.8), Some(Given::Steps(-1)), "with what was left");
    }

    #[test]
    fn an_encoder_moves_a_fader_from_where_it_stands_and_does_nothing_to_a_key() {
        let mut inputs = Inputs::default();
        let mut turn =
            |id, steps, step, at| inputs.resolve("DDJ", &target(id), Input::Steps { steps, step }, at);
        assert_eq!(turn("deck1.trim", 2.0, None, Some(0.5)), Some(Given::Value(0.5 + 2.0 / 64.0)));
        assert_eq!(
            turn("deck1.trim", -3.0, Some(0.25), Some(0.5)),
            Some(Given::Value(0.0)),
            "within its travel"
        );
        assert_eq!(turn("deck1.play", 1.0, None, None), None);
    }

    /// A controller's fader sending each of `values` in turn to the setting of `state`,
    /// standing at its `position`, given with `give`.
    fn turn<S>(state: &mut S, values: &[f32], position: impl Fn(&S) -> f32, give: impl Fn(&mut S, f32)) {
        let mut inputs = Inputs::default();
        let fader = target("deck1.eq.low");
        for &value in values {
            let at = Some(position(state));
            if let Some(Given::Value(v)) = inputs.resolve("DDJ", &fader, Input::Value(value), at) {
                give(state, v);
            }
        }
    }

    #[test]
    fn an_eq_band_and_a_live_input_come_back_from_their_silent_end() {
        type Scale = fn(f32) -> f32;
        let scales: [(Scale, Scale, f32); 2] = [(eq_gain, eq_knob, 0.5), (live_gain, live_knob, 0.75)];
        for (gain_of, knob_of, to) in scales {
            let mut fader = (0.0, SilentEnd::default());
            let position = |(gain, memory): &(f32, SilentEnd)| memory.position(knob_of(*gain));
            let give = |(gain, memory): &mut (f32, SilentEnd), v| {
                *gain = gain_of(v);
                memory.give(v, knob_of(*gain));
            };
            turn(&mut fader, &knob(to), position, give);
            assert!((fader.0 - 1.0).abs() < 0.1, "{}", fader.0);
        }

        // set to silent elsewhere: an encoder brings it back from there
        let mut inputs = Inputs::default();
        let (mut gain, mut memory) = (0.0, SilentEnd::default());
        let band = target("deck1.eq.low");
        for _ in 0..4 {
            let position = Some(memory.position(eq_knob(gain)));
            let step = Input::Steps { steps: 1.0, step: None };
            if let Some(Given::Value(v)) = inputs.resolve("DDJ", &band, step, position) {
                gain = eq_gain(v);
                memory.give(v, eq_knob(gain));
            }
        }
        assert!(gain > 0.0);
    }

    #[test]
    fn a_stepped_fader_keeps_a_controller_within_a_step() {
        // the periods of an effect, 1/8 to 8 beats: 7 steps
        let mut period = Stepped::default();
        assert!((period.position(3, 7) - 3.5 / 7.0).abs() < 1e-6);
        assert_eq!(period.give(1.0, 7), 6);
        assert_eq!(period.give(0.2, 7), 1);
        assert_eq!(period.position(1, 7), 0.2);
        // changed elsewhere, to 2 beats
        assert!((period.position(4, 7) - 4.5 / 7.0).abs() < 1e-6);
        // the 8 effects
        let mut kind = Stepped::default();
        assert_eq!(kind.give(0.99, 8), 7);
        assert_eq!(kind.give(0.4, 8), 3);
        assert_eq!(kind.position(3, 8), 0.4);
    }

    #[test]
    fn a_key_fader_and_an_encoder_reach_every_semitone() {
        let key = target("deck1.key");
        // a fader moved slowly up from the middle, its key read back as it stands
        let mut faders = Faders::default();
        let mut inputs = Inputs::default();
        let mut shift = 12;
        for value in (64..=127).map(|i| i as f32 / 127.0) {
            let at = Some(faders.stepped(&key).position(shift, KEY_STEPS));
            if let Some(Given::Value(v)) = inputs.resolve("DDJ", &key, Input::Value(value), at) {
                shift = faders.give_stepped(&key, v, KEY_STEPS);
            }
        }
        assert_eq!(shift, KEY_STEPS - 1, "12 semitones up");
        // an encoder, a step at a time from the middle
        let (mut faders, mut inputs, mut shift) = (Faders::default(), Inputs::default(), 12);
        let mut keys = Vec::new();
        for _ in 0..8 {
            let at = Some(faders.stepped(&key).position(shift, KEY_STEPS));
            let step = Input::Steps { steps: 1.0, step: None };
            if let Some(Given::Value(v)) = inputs.resolve("DDJ", &key, step, at) {
                shift = faders.give_stepped(&key, v, KEY_STEPS);
            }
            keys.push(shift);
        }
        assert!(keys.windows(2).all(|k| k[1] <= k[0] + 1), "a semitone at most a step: {keys:?}");
        assert!(shift >= 14, "{keys:?}");
    }

    #[test]
    fn the_crossfader_curve_switches_along_a_knob_both_ways() {
        let mut curve = (0, Stepped::default());
        let position = |(step, memory): &(usize, Stepped)| memory.position(*step, 2);
        let give = |(step, memory): &mut (usize, Stepped), v| *step = memory.give(v, 2);
        turn(&mut curve, &knob(1.0), position, give);
        assert_eq!(curve.0, 1);
        let back: Vec<f32> = knob(1.0).into_iter().rev().collect();
        turn(&mut curve, &back, position, give);
        assert_eq!(curve.0, 0);
    }
}
