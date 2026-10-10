use crate::message::{Control, ControlKind, Event};
use serde::{Deserialize, Serialize};

/// How a control's values are read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Encoding {
    /// A position, from 0 to 1 (a button: 1 pressed, 0 released).
    #[default]
    Absolute,
    /// An endless encoder: 1 to 63 are steps to the right, 65 to 127 steps to the left
    /// (127 = one step).
    Relative,
}

impl Encoding {
    pub fn value(self, event: Event) -> f32 {
        let raw = f32::from(event.raw);
        match (event.control.kind, self) {
            (ControlKind::PitchBend, _) => raw / 16383.0,
            (_, Encoding::Absolute) => raw / 127.0,
            (_, Encoding::Relative) if event.raw < 64 => raw,
            (_, Encoding::Relative) => raw - 128.0,
        }
    }
}

/// A control of a controller driving one of the application's actions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mapping {
    /// The controller's MIDI port name.
    pub device: String,
    pub control: Control,
    pub target: String,
    #[serde(default)]
    pub encoding: Encoding,
}

/// The mappings of all the controllers: on each controller, a control drives one action
/// and an action has one control.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Mappings(Vec<Mapping>);

impl Mappings {
    pub fn all(&self) -> &[Mapping] {
        &self.0
    }

    /// The action an event of `device` drives, and its value.
    pub fn resolve(&self, device: &str, event: Event) -> Option<(&str, f32)> {
        self.0
            .iter()
            .find(|m| m.device == device && m.control == event.control)
            .map(|m| (m.target.as_str(), m.encoding.value(event)))
    }

    /// Adds `mapping`, replacing what its control did and the control its action had on
    /// that controller.
    pub fn learn(&mut self, mapping: Mapping) {
        self.0.retain(|m| {
            m.device != mapping.device || (m.control != mapping.control && m.target != mapping.target)
        });
        self.0.push(mapping);
    }

    /// Removes the controls of `target`, on every controller.
    pub fn forget(&mut self, target: &str) {
        self.0.retain(|m| m.target != target);
    }
}

/// The message that switches a control's light on or off, as most controllers expect it:
/// the control's own note or controller, at 127 or 0.
pub fn light(control: Control, on: bool) -> Option<[u8; 3]> {
    let value = if on { 127 } else { 0 };
    match control.kind {
        ControlKind::Note => Some([0x90 | control.channel, control.number, value]),
        ControlKind::ControlChange => Some([0xb0 | control.channel, control.number, value]),
        ControlKind::PitchBend => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::parse;

    const PAD: Control = Control { kind: ControlKind::Note, channel: 0, number: 36 };
    const KNOB: Control = Control { kind: ControlKind::ControlChange, channel: 0, number: 20 };

    fn mapping(device: &str, control: Control, target: &str) -> Mapping {
        Mapping { device: device.into(), control, target: target.into(), encoding: Encoding::Absolute }
    }

    #[test]
    fn values_by_encoding() {
        let at = |raw| Event { control: KNOB, raw };
        assert_eq!(Encoding::Absolute.value(at(127)), 1.0);
        assert_eq!(Encoding::Absolute.value(at(0)), 0.0);
        assert_eq!(Encoding::Relative.value(at(3)), 3.0);
        assert_eq!(Encoding::Relative.value(at(127)), -1.0);
        assert_eq!(Encoding::Relative.value(at(65)), -63.0);
        let bend = parse(&[0xe0, 0x00, 0x40]).unwrap();
        assert_eq!(Encoding::Absolute.value(bend), 8192.0 / 16383.0);
    }

    #[test]
    fn events_drive_the_action_of_their_device() {
        let mut mappings = Mappings::default();
        mappings.learn(mapping("DDJ", PAD, "deck1.play"));
        mappings.learn(mapping("Pads", PAD, "deck2.play"));
        let press = Event { control: PAD, raw: 127 };
        assert_eq!(mappings.resolve("DDJ", press), Some(("deck1.play", 1.0)));
        assert_eq!(mappings.resolve("Pads", press), Some(("deck2.play", 1.0)));
        assert_eq!(mappings.resolve("Other", press), None);
        assert_eq!(mappings.resolve("DDJ", Event { control: KNOB, raw: 1 }), None);
    }

    #[test]
    fn learning_replaces_the_control_and_the_action() {
        let mut mappings = Mappings::default();
        mappings.learn(mapping("DDJ", PAD, "deck1.play"));
        mappings.learn(mapping("DDJ", KNOB, "deck1.play"));
        assert_eq!(mappings.all(), [mapping("DDJ", KNOB, "deck1.play")]);
        mappings.learn(mapping("DDJ", KNOB, "deck1.volume"));
        assert_eq!(mappings.all(), [mapping("DDJ", KNOB, "deck1.volume")]);
        mappings.learn(mapping("Pads", PAD, "deck1.volume"));
        mappings.forget("deck1.volume");
        assert!(mappings.all().is_empty());
    }

    #[test]
    fn lights_answer_on_the_same_control() {
        assert_eq!(light(Control { channel: 2, ..PAD }, true), Some([0x92, 36, 127]));
        assert_eq!(light(KNOB, false), Some([0xb0, 20, 0]));
        assert_eq!(light(Control { kind: ControlKind::PitchBend, channel: 0, number: 0 }, true), None);
    }

    #[test]
    fn mappings_survive_a_round_trip() {
        let mut mappings = Mappings::default();
        mappings.learn(Mapping { encoding: Encoding::Relative, ..mapping("DDJ", KNOB, "library.browse") });
        let json = serde_json::to_string(&mappings).unwrap();
        assert_eq!(serde_json::from_str::<Mappings>(&json).unwrap(), mappings);
    }
}
