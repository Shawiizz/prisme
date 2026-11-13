use crate::message::{Control, ControlKind, Event};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The action that turns a controller's other controls into their second layer while held.
pub const SHIFT: &str = "shift";

/// The end of the actions that take several controls of a controller: a jog wheel sends
/// its top and its edge apart.
const JOG: &str = ".jog";

/// How a control's values are read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Encoding {
    /// A position, from 0 to 1 (a button: 1 pressed, 0 released).
    #[default]
    Absolute,
    /// An endless encoder sending steps in two's complement: 1 to 63 to the right, 127
    /// (one step) down to 65 to the left.
    Relative,
    /// Steps around 64: 65 is one step to the right, 63 one to the left (Pioneer, Numark).
    RelativeOffset,
    /// Steps with a sign bit: 1 to 63 to the right, 65 (one step) to 127 to the left.
    RelativeSigned,
}

impl Encoding {
    pub fn is_relative(self) -> bool {
        self != Encoding::Absolute
    }

    pub fn value(self, event: Event) -> f32 {
        let raw = event.raw;
        let steps = |s: i32| s as f32;
        match (event.control.kind, self) {
            (ControlKind::PitchBend, _) => f32::from(raw) / 16383.0,
            (_, Encoding::Absolute) => f32::from(raw) / 127.0,
            (_, Encoding::Relative) if raw < 64 => steps(i32::from(raw)),
            (_, Encoding::Relative) => steps(i32::from(raw) - 128),
            (_, Encoding::RelativeOffset) => steps(i32::from(raw) - 64),
            (_, Encoding::RelativeSigned) if raw & 0x40 != 0 => steps(-i32::from(raw & 0x3f)),
            (_, Encoding::RelativeSigned) => steps(i32::from(raw)),
        }
    }

    /// The likeliest encoding of an encoder from its first step: values near 64 are
    /// centred steps, values near 0 or 127 two's complement ones. A sign-bit encoder looks
    /// like one of them: it is chosen by hand.
    pub fn guess(raw: u16) -> Encoding {
        if (32..96).contains(&raw) {
            Encoding::RelativeOffset
        } else {
            Encoding::Relative
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
    /// The controller sending the low 7 bits of a 14-bit control (its high 7 bits on
    /// `control`, by convention the number 32 lower).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fine: Option<u8>,
    /// Drives its action while the controller's shift is held (its second layer).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shift: bool,
}

impl Mapping {
    /// Whether `control` sends this mapping's values.
    fn sent_by(&self, control: Control) -> bool {
        self.control == control || self.fine_control() == Some(control)
    }

    fn fine_control(&self) -> Option<Control> {
        self.fine.map(|number| Control { number, ..self.control })
    }
}

/// The mappings of all the controllers: on each controller and layer, a control drives
/// one action, and an action has one control (a jog wheel, its top and its edge).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Mappings(Vec<Mapping>);

impl Mappings {
    pub fn all(&self) -> &[Mapping] {
        &self.0
    }

    /// The mapping an event of `device` belongs to: in the shift layer while shift is held
    /// (else in the first layer, as a control without a second use keeps its first).
    pub fn find(&self, device: &str, control: Control, shifted: bool) -> Option<&Mapping> {
        let find = |shift: bool| {
            self.0.iter().find(|m| m.device == device && m.shift == shift && m.sent_by(control))
        };
        find(shifted).or_else(|| find(false).filter(|_| shifted))
    }

    /// Adds `mapping`, replacing what its control did in its layer and the control its
    /// action had on that controller (a jog wheel keeps its other one).
    pub fn learn(&mut self, mapping: Mapping) {
        let several = mapping.target.ends_with(JOG);
        self.0.retain(|m| {
            let same_control = m.control == mapping.control && m.shift == mapping.shift;
            let replaced = same_control || (m.target == mapping.target && !several);
            m.device != mapping.device || !replaced
        });
        self.0.push(mapping);
    }

    /// Makes `mapping`'s control a 14-bit one, its low half on `fine`.
    pub fn make_fine(&mut self, mapping: &Mapping, fine: u8) {
        if let Some(m) = self.0.iter_mut().find(|m| *m == mapping) {
            m.fine = Some(fine);
        }
    }

    /// Changes how the control of `target` on `device` is read.
    pub fn set_encoding(&mut self, device: &str, target: &str, encoding: Encoding) {
        for m in self.0.iter_mut().filter(|m| m.device == device && m.target == target) {
            m.encoding = encoding;
        }
    }

    /// Removes the controls of `target`, on every controller.
    pub fn forget(&mut self, target: &str) {
        self.0.retain(|m| m.target != target);
    }
}

/// Turns events into values: holds the high half of 14-bit controls until their low half
/// comes, whether each controller's shift is held, and the layer each key held down was
/// pressed in (its release ends that press, wherever shift is by then).
#[derive(Debug, Default)]
pub struct Reader {
    coarse: HashMap<(String, Control), u16>,
    shifted: HashMap<String, bool>,
    pressed: HashMap<(String, Control), bool>,
}

impl Reader {
    /// The action an event of `device` drives and its value; `None` when it drives none
    /// or is the first half of a 14-bit value. Shift itself is read here.
    pub fn read<'m>(
        &mut self,
        mappings: &'m Mappings,
        device: &str,
        event: Event,
    ) -> Option<(&'m Mapping, f32)> {
        let key = event.control.kind == ControlKind::Note;
        let released = key && event.raw == 0;
        let layer = released.then(|| self.pressed.remove(&(device.to_owned(), event.control))).flatten();
        let shifted = layer.unwrap_or_else(|| self.shifted(device));
        let mapping = mappings.find(device, event.control, shifted)?;
        if key && !released {
            self.pressed.insert((device.to_owned(), event.control), shifted);
        }
        if mapping.target == SHIFT {
            self.shifted.insert(device.to_owned(), event.raw > 0);
            return None;
        }
        match mapping.fine_control() {
            Some(fine) if event.control == fine => {
                let coarse = self.coarse.get(&(device.to_owned(), mapping.control)).copied().unwrap_or(0);
                Some((mapping, f32::from(coarse << 7 | event.raw) / 16383.0))
            }
            Some(_) => {
                self.coarse.insert((device.to_owned(), mapping.control), event.raw);
                None
            }
            None => Some((mapping, mapping.encoding.value(event))),
        }
    }

    /// Whether the shift of `device` is held.
    pub fn shifted(&self, device: &str) -> bool {
        self.shifted.get(device).copied().unwrap_or(false)
    }

    /// The keys of `device` held down.
    pub fn held(&self, device: &str) -> Vec<Control> {
        self.pressed.keys().filter(|(d, _)| d == device).map(|(_, control)| *control).collect()
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
    const SHIFT_KEY: Control = Control { kind: ControlKind::Note, channel: 0, number: 63 };

    fn mapping(device: &str, control: Control, target: &str) -> Mapping {
        Mapping {
            device: device.into(),
            control,
            target: target.into(),
            encoding: Encoding::Absolute,
            fine: None,
            shift: false,
        }
    }

    fn read(reader: &mut Reader, mappings: &Mappings, bytes: &[u8]) -> Option<(String, f32)> {
        let event = parse(bytes)?;
        reader.read(mappings, "DDJ", event).map(|(m, v)| (m.target.clone(), v))
    }

    #[test]
    fn values_by_encoding() {
        let at = |raw| Event { control: KNOB, raw };
        assert_eq!(Encoding::Absolute.value(at(127)), 1.0);
        assert_eq!(Encoding::Absolute.value(at(0)), 0.0);
        assert_eq!(Encoding::Relative.value(at(3)), 3.0);
        assert_eq!(Encoding::Relative.value(at(127)), -1.0);
        assert_eq!(Encoding::Relative.value(at(65)), -63.0);
        assert_eq!(Encoding::RelativeOffset.value(at(65)), 1.0);
        assert_eq!(Encoding::RelativeOffset.value(at(60)), -4.0);
        assert_eq!(Encoding::RelativeSigned.value(at(2)), 2.0);
        assert_eq!(Encoding::RelativeSigned.value(at(65)), -1.0);
        let bend = parse(&[0xe0, 0x00, 0x40]).unwrap();
        assert_eq!(Encoding::Absolute.value(bend), 8192.0 / 16383.0);
    }

    #[test]
    fn an_encoder_is_guessed_from_its_first_step() {
        assert_eq!(Encoding::guess(1), Encoding::Relative);
        assert_eq!(Encoding::guess(127), Encoding::Relative);
        assert_eq!(Encoding::guess(65), Encoding::RelativeOffset);
        assert_eq!(Encoding::guess(62), Encoding::RelativeOffset);
    }

    #[test]
    fn events_drive_the_action_of_their_device() {
        let mut mappings = Mappings::default();
        mappings.learn(mapping("DDJ", PAD, "deck1.play"));
        mappings.learn(mapping("Pads", PAD, "deck2.play"));
        let mut reader = Reader::default();
        let press = Event { control: PAD, raw: 127 };
        let target = |reader: &mut Reader, device| {
            reader.read(&mappings, device, press).map(|(m, _)| m.target.clone())
        };
        assert_eq!(target(&mut reader, "DDJ").as_deref(), Some("deck1.play"));
        assert_eq!(target(&mut reader, "Pads").as_deref(), Some("deck2.play"));
        assert_eq!(target(&mut reader, "Other"), None);
    }

    #[test]
    fn a_fine_control_completes_its_coarse_half() {
        let mut mappings = Mappings::default();
        let tempo = mapping("DDJ", Control { number: 0, ..KNOB }, "deck1.pitch");
        mappings.learn(tempo.clone());
        mappings.make_fine(&tempo, 32);
        let mut reader = Reader::default();
        assert_eq!(read(&mut reader, &mappings, &[0xb0, 0, 64]), None);
        assert_eq!(
            read(&mut reader, &mappings, &[0xb0, 32, 1]),
            Some(("deck1.pitch".into(), 8193.0 / 16383.0))
        );
        assert_eq!(
            read(&mut reader, &mappings, &[0xb0, 32, 127]),
            Some(("deck1.pitch".into(), 8319.0 / 16383.0))
        );
    }

    #[test]
    fn shift_selects_the_second_layer_and_falls_back_on_the_first() {
        let mut mappings = Mappings::default();
        mappings.learn(mapping("DDJ", SHIFT_KEY, SHIFT));
        mappings.learn(mapping("DDJ", PAD, "deck1.hotcue1"));
        mappings.learn(Mapping { shift: true, ..mapping("DDJ", PAD, "deck1.hotcue1.delete") });
        mappings.learn(mapping("DDJ", KNOB, "deck1.volume"));
        let mut reader = Reader::default();
        assert_eq!(read(&mut reader, &mappings, &[0x90, 36, 127]).unwrap().0, "deck1.hotcue1");
        assert_eq!(read(&mut reader, &mappings, &[0x90, 63, 127]), None);
        assert!(reader.shifted("DDJ"));
        assert_eq!(read(&mut reader, &mappings, &[0x90, 36, 127]).unwrap().0, "deck1.hotcue1.delete");
        assert_eq!(read(&mut reader, &mappings, &[0xb0, 20, 64]).unwrap().0, "deck1.volume");
        read(&mut reader, &mappings, &[0x90, 63, 0]);
        assert!(!reader.shifted("DDJ"));
        assert_eq!(read(&mut reader, &mappings, &[0x90, 36, 127]).unwrap().0, "deck1.hotcue1");
    }

    #[test]
    fn a_key_is_released_in_the_layer_it_was_pressed_in() {
        let mut mappings = Mappings::default();
        mappings.learn(mapping("DDJ", SHIFT_KEY, SHIFT));
        mappings.learn(mapping("DDJ", PAD, "deck1.cue"));
        mappings.learn(Mapping { shift: true, ..mapping("DDJ", PAD, "deck1.censor") });
        let mut reader = Reader::default();
        read(&mut reader, &mappings, &[0x90, 63, 127]);
        assert_eq!(read(&mut reader, &mappings, &[0x90, 36, 90]).unwrap().0, "deck1.censor");
        assert_eq!(reader.held("DDJ").len(), 2);
        // shift let go first: the pad still ends what it began
        read(&mut reader, &mappings, &[0x90, 63, 0]);
        assert_eq!(read(&mut reader, &mappings, &[0x80, 36, 0]), Some(("deck1.censor".into(), 0.0)));
        assert!(reader.held("DDJ").is_empty());
        // and the other way round: pressed plain, released under shift
        read(&mut reader, &mappings, &[0x90, 36, 90]);
        read(&mut reader, &mappings, &[0x90, 63, 127]);
        assert_eq!(read(&mut reader, &mappings, &[0x90, 36, 0]), Some(("deck1.cue".into(), 0.0)));
    }

    #[test]
    fn a_jog_wheel_keeps_its_top_and_its_edge() {
        let edge = Control { number: 33, ..KNOB };
        let mut mappings = Mappings::default();
        mappings.learn(mapping("DDJ", KNOB, "deck1.jog"));
        mappings.learn(mapping("DDJ", edge, "deck1.jog"));
        assert_eq!(mappings.all().len(), 2);
        // the same control learnt again is not there twice, and still drives one action
        mappings.learn(mapping("DDJ", edge, "deck1.jog"));
        assert_eq!(mappings.all().len(), 2);
        mappings.learn(mapping("DDJ", edge, "deck2.jog"));
        assert_eq!(mappings.all(), [mapping("DDJ", KNOB, "deck1.jog"), mapping("DDJ", edge, "deck2.jog")]);
    }

    #[test]
    fn learning_replaces_the_control_and_the_action() {
        let mut mappings = Mappings::default();
        mappings.learn(mapping("DDJ", PAD, "deck1.play"));
        mappings.learn(mapping("DDJ", KNOB, "deck1.play"));
        assert_eq!(mappings.all(), [mapping("DDJ", KNOB, "deck1.play")]);
        mappings.learn(mapping("DDJ", KNOB, "deck1.volume"));
        assert_eq!(mappings.all(), [mapping("DDJ", KNOB, "deck1.volume")]);
        // the same control in the shift layer is another use of it
        mappings.learn(Mapping { shift: true, ..mapping("DDJ", KNOB, "deck1.filter") });
        assert_eq!(mappings.all().len(), 2);
        mappings.learn(mapping("Pads", PAD, "deck1.volume"));
        mappings.forget("deck1.volume");
        assert_eq!(mappings.all().len(), 1);
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
        mappings.learn(Mapping { encoding: Encoding::RelativeOffset, ..mapping("DDJ", KNOB, "deck1.jog") });
        mappings.learn(Mapping { fine: Some(32), shift: true, ..mapping("DDJ", PAD, "deck1.pitch") });
        let json = serde_json::to_string(&mappings).unwrap();
        assert_eq!(serde_json::from_str::<Mappings>(&json).unwrap(), mappings);
        // mappings saved before 14-bit controls and layers existed
        let old =
            r#"[{"device":"DDJ","control":{"kind":"note","channel":0,"number":36},"target":"deck1.play"}]"#;
        assert_eq!(serde_json::from_str::<Mappings>(old).unwrap().all(), [mapping("DDJ", PAD, "deck1.play")]);
    }
}
