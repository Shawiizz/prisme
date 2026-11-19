use crate::hid::Field;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A physical control of a controller: what one of its MIDI messages addresses, or a field of
/// one of its HID reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Control {
    /// A note: buttons and pads.
    Note(Midi),
    /// A controller: knobs, faders, encoders (and the buttons of some controllers).
    ControlChange(Midi),
    /// The pitch bend, its number 0.
    PitchBend(Midi),
    /// A field of a HID report.
    Hid(Field),
}

/// Where a MIDI message goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Midi {
    /// 0 to 15.
    #[schemars(range(max = 15))]
    pub channel: u8,
    /// Note or controller number; 0 for the pitch bend.
    #[schemars(range(max = 127))]
    pub number: u8,
}

impl Default for Control {
    fn default() -> Self {
        Control::Note(Midi::default())
    }
}

impl Control {
    pub const fn note(channel: u8, number: u8) -> Self {
        Control::Note(Midi { channel, number })
    }

    pub const fn control_change(channel: u8, number: u8) -> Self {
        Control::ControlChange(Midi { channel, number })
    }

    pub const fn pitch_bend(channel: u8) -> Self {
        Control::PitchBend(Midi { channel, number: 0 })
    }

    /// Whether it is a key: pressed while its value is above 0, let go at 0.
    pub fn is_key(self) -> bool {
        match self {
            Control::Note(_) => true,
            Control::Hid(field) => field.bits == 1,
            Control::ControlChange(_) | Control::PitchBend(_) => false,
        }
    }

    /// How many bits its values have.
    pub fn bits(self) -> u8 {
        match self {
            Control::Note(_) | Control::ControlChange(_) => 7,
            Control::PitchBend(_) => 14,
            Control::Hid(field) => field.bits,
        }
    }

    /// The controller numbered `number` on the same channel, if it is a controller.
    pub fn with_number(self, number: u8) -> Option<Self> {
        match self {
            Control::ControlChange(midi) => Some(Control::ControlChange(Midi { number, ..midi })),
            _ => None,
        }
    }
}

/// `note 36, channel 1`, `report 1, byte 13, bit 2`, `report 2, byte 9, 12 bits`.
impl std::fmt::Display for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Control::Note(m) => write!(f, "note {}, channel {}", m.number, m.channel + 1),
            Control::ControlChange(m) => write!(f, "controller {}, channel {}", m.number, m.channel + 1),
            Control::PitchBend(m) => write!(f, "pitch bend, channel {}", m.channel + 1),
            Control::Hid(field) if field.bits == 1 => {
                write!(f, "report {}, byte {}, bit {}", field.report, field.byte, field.bit)
            }
            Control::Hid(field) => {
                write!(f, "report {}, byte {}, {} bits", field.report, field.byte, field.bits)
            }
        }
    }
}

/// A control moved or was pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    pub control: Control,
    /// Its value, of [`Control::bits`] bits: 0 to 127 (a released note is 0), 0 to 16383 for
    /// the pitch bend.
    pub raw: u16,
}

/// Reads a MIDI message; `None` for the messages controls do not send (clock, SysEx…).
pub fn parse(bytes: &[u8]) -> Option<Event> {
    let (&status, data) = bytes.split_first()?;
    let byte = |i: usize| data.get(i).copied().filter(|b| b & 0x80 == 0);
    let channel = status & 0x0f;
    let (control, raw) = match status & 0xf0 {
        0x80 => (Control::note(channel, byte(0)?), 0),
        0x90 => (Control::note(channel, byte(0)?), u16::from(byte(1)?)),
        0xb0 => (Control::control_change(channel, byte(0)?), u16::from(byte(1)?)),
        0xe0 => (Control::pitch_bend(channel), u16::from(byte(0)?) | u16::from(byte(1)?) << 7),
        _ => return None,
    };
    Some(Event { control, raw })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_pressed_and_released() {
        let pad = Control::note(1, 36);
        assert_eq!(parse(&[0x91, 36, 100]), Some(Event { control: pad, raw: 100 }));
        assert_eq!(parse(&[0x81, 36, 64]), Some(Event { control: pad, raw: 0 }));
        // a note on at velocity 0 is how many controllers release
        assert_eq!(parse(&[0x91, 36, 0]), Some(Event { control: pad, raw: 0 }));
    }

    #[test]
    fn controllers_and_pitch_bend() {
        assert_eq!(parse(&[0xb0, 7, 127]), Some(Event { control: Control::control_change(0, 7), raw: 127 }));
        assert_eq!(parse(&[0xef, 0x7f, 0x7f]), Some(Event { control: Control::pitch_bend(15), raw: 16383 }));
    }

    #[test]
    fn other_messages_are_ignored() {
        assert_eq!(parse(&[0xf8]), None);
        assert_eq!(parse(&[0xf0, 0x7e, 0x7f, 0xf7]), None);
        assert_eq!(parse(&[0xc0, 5]), None);
        assert_eq!(parse(&[0x90, 36]), None);
        assert_eq!(parse(&[0xb0, 0x80, 1]), None);
        assert_eq!(parse(&[]), None);
    }

    #[test]
    fn a_control_is_written_as_before_and_a_hid_field_by_its_place() {
        let json = r#"{"kind":"controlChange","channel":2,"number":7}"#;
        let knob: Control = serde_json::from_str(json).unwrap();
        assert_eq!(knob, Control::control_change(2, 7));
        assert_eq!(serde_json::to_string(&knob).unwrap(), json);
        let key: Control =
            serde_json::from_str(r#"{ "kind": "hid", "report": 1, "byte": 13, "bit": 2 }"#).unwrap();
        assert!(key.is_key());
        let fader: Control =
            serde_json::from_str(r#"{ "kind": "hid", "report": 2, "byte": 9, "bits": 12 }"#).unwrap();
        assert!(!fader.is_key() && fader.bits() == 12);
        assert!(!Control::control_change(0, 1).is_key());
        assert_eq!(Control::pitch_bend(0).bits(), 14);
    }
}
