use serde::{Deserialize, Serialize};

/// A physical control of a controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Control {
    pub kind: ControlKind,
    /// 0 to 15.
    pub channel: u8,
    /// Note or controller number; 0 for the pitch bend.
    pub number: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ControlKind {
    /// Buttons and pads.
    Note,
    /// Knobs, faders, encoders (and the buttons of some controllers).
    ControlChange,
    PitchBend,
}

/// A control moved or was pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    pub control: Control,
    /// 0 to 127 (a released note is 0), 0 to 16383 for the pitch bend.
    pub raw: u16,
}

/// Reads a MIDI message; `None` for the messages controls do not send (clock, SysEx…).
pub fn parse(bytes: &[u8]) -> Option<Event> {
    let (&status, data) = bytes.split_first()?;
    let byte = |i: usize| data.get(i).copied().filter(|b| b & 0x80 == 0);
    let (kind, number, raw) = match status & 0xf0 {
        0x80 => (ControlKind::Note, byte(0)?, 0),
        0x90 => (ControlKind::Note, byte(0)?, u16::from(byte(1)?)),
        0xb0 => (ControlKind::ControlChange, byte(0)?, u16::from(byte(1)?)),
        0xe0 => (ControlKind::PitchBend, 0, u16::from(byte(0)?) | u16::from(byte(1)?) << 7),
        _ => return None,
    };
    Some(Event { control: Control { kind, channel: status & 0x0f, number }, raw })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control(kind: ControlKind, channel: u8, number: u8) -> Control {
        Control { kind, channel, number }
    }

    #[test]
    fn notes_pressed_and_released() {
        let pad = control(ControlKind::Note, 1, 36);
        assert_eq!(parse(&[0x91, 36, 100]), Some(Event { control: pad, raw: 100 }));
        assert_eq!(parse(&[0x81, 36, 64]), Some(Event { control: pad, raw: 0 }));
        // a note on at velocity 0 is how many controllers release
        assert_eq!(parse(&[0x91, 36, 0]), Some(Event { control: pad, raw: 0 }));
    }

    #[test]
    fn controllers_and_pitch_bend() {
        assert_eq!(
            parse(&[0xb0, 7, 127]),
            Some(Event { control: control(ControlKind::ControlChange, 0, 7), raw: 127 })
        );
        assert_eq!(
            parse(&[0xef, 0x7f, 0x7f]),
            Some(Event { control: control(ControlKind::PitchBend, 15, 0), raw: 16383 })
        );
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
}
