use crate::message::{Control, ControlKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What a light shows for each state of its action: `on` and `off`, or the action's own
/// (`loaded`, `playing`…). A state it does not name shows as `on`, but `off`: 127 and 0
/// when it names neither.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LightValues(BTreeMap<String, LightValue>);

/// A light's value in a state: steady, or two in turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LightValue {
    Steady(u8),
    /// Each of the two values for `ms` milliseconds.
    Blink {
        blink: [u8; 2],
        ms: u32,
    },
}

pub const ON: &str = "on";
pub const OFF: &str = "off";

impl LightValues {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The value of `state`.
    pub fn value(&self, state: &str) -> LightValue {
        let off = state == OFF;
        let named = self.0.get(state).or_else(|| (!off).then(|| self.0.get(ON)).flatten());
        named.copied().unwrap_or(LightValue::Steady(if off { 0 } else { 127 }))
    }
}

impl FromIterator<(String, LightValue)> for LightValues {
    fn from_iter<I: IntoIterator<Item = (String, LightValue)>>(values: I) -> Self {
        Self(values.into_iter().collect())
    }
}

/// The message that sets a control's light to `value`, as controllers expect it: the
/// control's own note or controller.
pub fn light(control: Control, value: u8) -> Option<[u8; 3]> {
    match control.kind {
        ControlKind::Note => Some([0x90 | control.channel, control.number, value]),
        ControlKind::ControlChange => Some([0xb0 | control.channel, control.number, value]),
        ControlKind::PitchBend => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAD: Control = Control { kind: ControlKind::Note, channel: 0, number: 36 };

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
        assert_eq!(light(Control { channel: 2, ..PAD }, 127), Some([0x92, 36, 127]));
        let knob = Control { kind: ControlKind::ControlChange, ..PAD };
        assert_eq!(light(knob, 0), Some([0xb0, 36, 0]));
        assert_eq!(light(Control { kind: ControlKind::PitchBend, channel: 0, number: 0 }, 127), None);
    }
}
