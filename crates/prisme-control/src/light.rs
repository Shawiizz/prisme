use crate::message::Control;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What a light shows for each state of its action: `on` and `off`, or the action's own
/// (`loaded`, `playing`…). A state it does not name shows as `on`, but `off`: 127 and 0
/// when it names neither.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct LightValues(BTreeMap<String, LightValue>);

/// A light's value in a state: steady, two in turn, or a whole message of its own (for the
/// models addressing their lights otherwise than by a value on the control's note; of a HID
/// light, its bytes: the red, green and blue of a light of three colours).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum LightValue {
    Steady(u8),
    /// Each of the two values for `ms` milliseconds.
    Blink {
        blink: [u8; 2],
        ms: u32,
    },
    Message(Vec<u8>),
}

pub const ON: &str = "on";
pub const OFF: &str = "off";

impl LightValues {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether it names `state`.
    pub fn has(&self, state: &str) -> bool {
        self.0.contains_key(state)
    }

    /// The value of `state`.
    pub fn value(&self, state: &str) -> LightValue {
        let off = state == OFF;
        let named = self.0.get(state).or_else(|| (!off).then(|| self.0.get(ON)).flatten());
        named.cloned().unwrap_or(LightValue::Steady(if off { 0 } else { 127 }))
    }
}

impl FromIterator<(String, LightValue)> for LightValues {
    fn from_iter<I: IntoIterator<Item = (String, LightValue)>>(values: I) -> Self {
        Self(values.into_iter().collect())
    }
}

/// The value of `palette` (`"#rrggbb"`: value) whose colour is the nearest `colour`
/// (`0xrrggbb`).
pub fn nearest(palette: &BTreeMap<String, u8>, colour: u32) -> Option<u8> {
    let channels = |n: u32| [n >> 16 & 0xff, n >> 8 & 0xff, n & 0xff].map(|c| c as i32);
    let rgb = |hex: &str| u32::from_str_radix(hex.trim_start_matches('#'), 16).ok().map(channels);
    let wanted = channels(colour);
    let distance = |c: [i32; 3]| (0..3).map(|i| (c[i] - wanted[i]).pow(2)).sum::<i32>();
    palette.iter().filter_map(|(c, &value)| Some((distance(rgb(c)?), value))).min().map(|(_, value)| value)
}

/// The MIDI message that sets a control's light to `value`, as controllers expect it: the
/// control's own note or controller.
pub fn light(control: Control, value: u8) -> Option<[u8; 3]> {
    match control {
        Control::Note(midi) => Some([0x90 | midi.channel, midi.number, value]),
        Control::ControlChange(midi) => Some([0xb0 | midi.channel, midi.number, value]),
        Control::PitchBend(_) | Control::Hid(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAD: Control = Control::note(0, 36);

    #[test]
    fn a_light_without_values_is_full_or_dark() {
        let values = LightValues::default();
        assert_eq!(values.value(ON), LightValue::Steady(127));
        assert_eq!(values.value(OFF), LightValue::Steady(0));
        assert_eq!(values.value("playing"), LightValue::Steady(127));
    }

    #[test]
    fn a_state_it_does_not_name_shows_as_on() {
        let values: LightValues =
            serde_json::from_str(r#"{ "on": 100, "off": 1, "playing": { "blink": [9, 0], "ms": 250 } }"#)
                .unwrap();
        assert_eq!(values.value("loaded"), LightValue::Steady(100));
        assert_eq!(values.value(OFF), LightValue::Steady(1));
        assert_eq!(values.value("playing"), LightValue::Blink { blink: [9, 0], ms: 250 });
    }

    #[test]
    fn lights_answer_on_the_same_control() {
        assert_eq!(light(PAD, 127), Some([0x90, 36, 127]));
        assert_eq!(light(Control::note(2, 36), 127), Some([0x92, 36, 127]));
        assert_eq!(light(Control::control_change(0, 36), 0), Some([0xb0, 36, 0]));
        assert_eq!(light(Control::pitch_bend(0), 127), None);
    }

    #[test]
    fn a_light_value_is_a_value_two_in_turn_or_a_message() {
        let values: LightValues = serde_json::from_str(
            r#"{ "on": 5, "playing": { "blink": [5, 0], "ms": 250 }, "off": [176, 75, 12] }"#,
        )
        .unwrap();
        assert_eq!(values.value("on"), LightValue::Steady(5));
        assert_eq!(values.value("playing"), LightValue::Blink { blink: [5, 0], ms: 250 });
        assert_eq!(values.value("off"), LightValue::Message(vec![176, 75, 12]));
    }

    #[test]
    fn a_colour_takes_the_nearest_of_a_palette() {
        let palette = BTreeMap::from([("#ff0000".to_owned(), 5), ("#0000ff".to_owned(), 45)]);
        assert_eq!(nearest(&palette, 0xe01010), Some(5));
        assert_eq!(nearest(&palette, 0x2020c0), Some(45));
        assert_eq!(nearest(&BTreeMap::new(), 0x2020c0), None);
    }
}
