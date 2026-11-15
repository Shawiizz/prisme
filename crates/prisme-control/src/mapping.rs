use crate::light::LightValues;
use crate::message::{Control, ControlKind, Event};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The action that turns a controller's other controls into their second layer while held.
pub const SHIFT: &str = "shift";

/// The actions that put a controller's group of controls in a mode: `mode.<group>.<mode>`.
pub const MODE: &str = "mode.";

/// The mode of each group of a controller's controls (the page of a side's pads, say).
pub type Modes = HashMap<String, String>;

/// The group and the mode an action selects, if it is a mode's.
pub fn selected_mode(target: &str) -> Option<(&str, &str)> {
    target.strip_prefix(MODE)?.split_once('.')
}

/// A mode of a controller: `mode` of its group `group`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct When {
    pub group: String,
    pub mode: String,
}

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
    /// Drives its action only while its group is in this mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
    /// A fader whose lowest value is at its top.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub invert: bool,
    /// What its light shows in each state of its action.
    #[serde(default, skip_serializing_if = "LightValues::is_empty")]
    pub light: LightValues,
}

impl Mapping {
    /// Whether `control` sends this mapping's values.
    fn sent_by(&self, control: Control) -> bool {
        self.control == control || self.fine_control() == Some(control)
    }

    fn fine_control(&self) -> Option<Control> {
        self.fine.map(|number| Control { number, ..self.control })
    }

    /// Whether it drives its action in `modes`.
    pub fn applies(&self, modes: &Modes) -> bool {
        self.when.as_ref().is_none_or(|w| modes.get(&w.group) == Some(&w.mode))
    }

    /// Its value of a position from 0 to 1.
    fn position(&self, value: f32) -> f32 {
        if self.invert {
            1.0 - value
        } else {
            value
        }
    }
}

/// The mappings of all the controllers: on each controller and layer, a control drives
/// one action, and an action has one control, but for those the application says take
/// several (a jog wheel sends its top and its edge apart).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Mappings(Vec<Mapping>);

impl Mappings {
    pub fn all(&self) -> &[Mapping] {
        &self.0
    }

    /// The mapping an event of `device` belongs to in `modes`: in the shift layer while shift
    /// is held (else in the first layer, as a control without a second use keeps its first).
    pub fn find(&self, device: &str, control: Control, shifted: bool, modes: &Modes) -> Option<&Mapping> {
        let find = |shift: bool| {
            self.0
                .iter()
                .find(|m| m.device == device && m.shift == shift && m.applies(modes) && m.sent_by(control))
        };
        find(shifted).or_else(|| find(false).filter(|_| shifted))
    }

    /// Adds `mapping`, replacing what its control did in its layer and the control its
    /// action had on that controller, unless the action takes `several`.
    pub fn learn(&mut self, mapping: Mapping, several: bool) {
        self.0.retain(|m| {
            let same_control =
                m.control == mapping.control && m.shift == mapping.shift && m.when == mapping.when;
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

    /// The mappings of the controllers' models (their profiles) with those learnt over
    /// them: a control or an action learnt on a controller takes the model's place, and an
    /// action forgotten (`unmapped`) loses the model's controls too. `several` tells the
    /// actions that take several controls.
    pub fn layered(
        model: impl IntoIterator<Item = Mapping>,
        learnt: &Mappings,
        unmapped: &[String],
        several: impl Fn(&str) -> bool,
    ) -> Self {
        let mut layered = Mappings(model.into_iter().filter(|m| !unmapped.contains(&m.target)).collect());
        for mapping in learnt.all() {
            layered.learn(mapping.clone(), several(&mapping.target));
        }
        layered
    }
}

/// Turns events into values, controller by controller.
#[derive(Debug, Default)]
pub struct Reader {
    devices: HashMap<String, DeviceReader>,
}

/// What is read of a controller: the high half of its 14-bit controls until their low half
/// comes, whether its shift is held, the mode of each group of its controls, and the layer
/// and modes each key held down was pressed in (its release ends that press, wherever shift
/// and the modes are by then).
#[derive(Debug, Default)]
struct DeviceReader {
    coarse: HashMap<Control, u16>,
    shifted: bool,
    modes: Modes,
    /// How many times a mode key changed a mode.
    mode_changes: u32,
    pressed: HashMap<Control, (bool, Modes)>,
}

impl Reader {
    /// The action an event of `device` drives and its value; `None` when it drives none
    /// or is the first half of a 14-bit value. Shift and the modes are read here.
    pub fn read<'m>(
        &mut self,
        mappings: &'m Mappings,
        device: &str,
        event: Event,
    ) -> Option<(&'m Mapping, f32)> {
        let reader = self.device(device);
        let key = event.control.kind == ControlKind::Note;
        let released = key && event.raw == 0;
        let layer = released.then(|| reader.pressed.remove(&event.control)).flatten();
        let mapping = match &layer {
            Some((shifted, modes)) => mappings.find(device, event.control, *shifted, modes),
            None => mappings.find(device, event.control, reader.shifted, &reader.modes),
        }?;
        if key && !released {
            reader.pressed.insert(event.control, (reader.shifted, reader.modes.clone()));
        }
        if mapping.target == SHIFT {
            reader.shifted = event.raw > 0;
            return None;
        }
        if let Some((group, mode)) = selected_mode(&mapping.target) {
            if event.raw > 0 && reader.modes.get(group).is_none_or(|m| m != mode) {
                reader.modes.insert(group.to_owned(), mode.to_owned());
                reader.mode_changes = reader.mode_changes.wrapping_add(1);
            }
            return None;
        }
        match mapping.fine_control() {
            Some(fine) if event.control == fine => {
                let coarse = reader.coarse.get(&mapping.control).copied().unwrap_or(0);
                Some((mapping, mapping.position(f32::from(coarse << 7 | event.raw) / 16383.0)))
            }
            Some(_) => {
                reader.coarse.insert(mapping.control, event.raw);
                None
            }
            None if mapping.encoding.is_relative() => Some((mapping, mapping.encoding.value(event))),
            None => Some((mapping, mapping.position(mapping.encoding.value(event)))),
        }
    }

    /// Whether the shift of `device` is held.
    pub fn shifted(&self, device: &str) -> bool {
        self.devices.get(device).is_some_and(|d| d.shifted)
    }

    /// The modes `device`'s groups are in.
    pub fn modes(&self, device: &str) -> Modes {
        self.devices.get(device).map(|d| d.modes.clone()).unwrap_or_default()
    }

    /// How many times `device`'s mode keys changed a mode: a change shows in another count.
    pub fn mode_changes(&self, device: &str) -> u32 {
        self.devices.get(device).map_or(0, |d| d.mode_changes)
    }

    /// Puts `device`'s groups in `modes`, those it starts in.
    pub fn set_modes(&mut self, device: &str, modes: Modes) {
        self.device(device).modes = modes;
    }

    /// The keys of `device` held down.
    pub fn held(&self, device: &str) -> Vec<Control> {
        self.devices.get(device).map(|d| d.pressed.keys().copied().collect()).unwrap_or_default()
    }

    /// What is read of `device`: its name is copied once, on its first event, not on each.
    fn device(&mut self, device: &str) -> &mut DeviceReader {
        if !self.devices.contains_key(device) {
            self.devices.insert(device.to_owned(), DeviceReader::default());
        }
        self.devices.get_mut(device).expect("inserted above")
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
            when: None,
            invert: false,
            light: LightValues::default(),
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
        mappings.learn(mapping("DDJ", PAD, "deck1.play"), false);
        mappings.learn(mapping("Pads", PAD, "deck2.play"), false);
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
        mappings.learn(tempo.clone(), false);
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
        mappings.learn(mapping("DDJ", SHIFT_KEY, SHIFT), false);
        mappings.learn(mapping("DDJ", PAD, "deck1.hotcue1"), false);
        mappings.learn(Mapping { shift: true, ..mapping("DDJ", PAD, "deck1.hotcue1.delete") }, false);
        mappings.learn(mapping("DDJ", KNOB, "deck1.volume"), false);
        let mut reader = Reader::default();
        assert_eq!(read(&mut reader, &mappings, &[0x90, 36, 127]).unwrap().0, "deck1.hotcue1");
        assert_eq!(read(&mut reader, &mappings, &[0x90, 63, 127]), None);
        assert!(reader.shifted("DDJ"));
        assert!(!reader.shifted("Pads"), "each controller has its own shift");
        assert_eq!(read(&mut reader, &mappings, &[0x90, 36, 127]).unwrap().0, "deck1.hotcue1.delete");
        assert_eq!(read(&mut reader, &mappings, &[0xb0, 20, 64]).unwrap().0, "deck1.volume");
        read(&mut reader, &mappings, &[0x90, 63, 0]);
        assert!(!reader.shifted("DDJ"));
        assert_eq!(read(&mut reader, &mappings, &[0x90, 36, 127]).unwrap().0, "deck1.hotcue1");
    }

    #[test]
    fn a_key_is_released_in_the_layer_it_was_pressed_in() {
        let mut mappings = Mappings::default();
        mappings.learn(mapping("DDJ", SHIFT_KEY, SHIFT), false);
        mappings.learn(mapping("DDJ", PAD, "deck1.cue"), false);
        mappings.learn(Mapping { shift: true, ..mapping("DDJ", PAD, "deck1.censor") }, false);
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
        mappings.learn(mapping("DDJ", KNOB, "deck1.jog"), true);
        mappings.learn(mapping("DDJ", edge, "deck1.jog"), true);
        assert_eq!(mappings.all().len(), 2);
        // the same control learnt again is not there twice, and still drives one action
        mappings.learn(mapping("DDJ", edge, "deck1.jog"), true);
        assert_eq!(mappings.all().len(), 2);
        mappings.learn(mapping("DDJ", edge, "deck2.jog"), true);
        assert_eq!(mappings.all(), [mapping("DDJ", KNOB, "deck1.jog"), mapping("DDJ", edge, "deck2.jog")]);
    }

    #[test]
    fn learning_replaces_the_control_and_the_action() {
        let mut mappings = Mappings::default();
        mappings.learn(mapping("DDJ", PAD, "deck1.play"), false);
        mappings.learn(mapping("DDJ", KNOB, "deck1.play"), false);
        assert_eq!(mappings.all(), [mapping("DDJ", KNOB, "deck1.play")]);
        mappings.learn(mapping("DDJ", KNOB, "deck1.volume"), false);
        assert_eq!(mappings.all(), [mapping("DDJ", KNOB, "deck1.volume")]);
        // the same control in the shift layer is another use of it
        mappings.learn(Mapping { shift: true, ..mapping("DDJ", KNOB, "deck1.filter") }, false);
        assert_eq!(mappings.all().len(), 2);
        mappings.learn(mapping("Pads", PAD, "deck1.volume"), false);
        mappings.forget("deck1.volume");
        assert_eq!(mappings.all().len(), 1);
    }

    #[test]
    fn learnt_mappings_take_the_place_of_the_model() {
        let shifted_pad = Control { number: 37, ..PAD };
        let model = [
            mapping("DDJ", PAD, "deck1.play"),
            mapping("DDJ", shifted_pad, "deck1.play"),
            mapping("DDJ", KNOB, "deck1.volume"),
            mapping("DDJ", SHIFT_KEY, "deck1.sync"),
        ];
        let mut learnt = Mappings::default();
        // the pad learnt for another action, the knob's action learnt on another control
        learnt.learn(mapping("DDJ", PAD, "deck1.cue"), false);
        learnt.learn(mapping("DDJ", Control { number: 21, ..KNOB }, "deck1.volume"), false);
        let layered = Mappings::layered(model, &learnt, &["deck1.sync".to_owned()], |_| false);
        let target = |control| layered.find("DDJ", control, false, &Modes::new()).map(|m| m.target.as_str());
        assert_eq!(target(PAD), Some("deck1.cue"));
        assert_eq!(target(shifted_pad), Some("deck1.play"));
        assert_eq!(target(KNOB), None);
        assert_eq!(target(Control { number: 21, ..KNOB }), Some("deck1.volume"));
        assert_eq!(target(SHIFT_KEY), None, "forgotten");
    }

    #[test]
    fn a_mode_selects_what_its_controls_drive() {
        let mode = |name: &str| When { group: "padsA".into(), mode: name.into() };
        let roll_key = Control { number: 7, ..PAD };
        let mut mappings = Mappings::default();
        mappings.learn(mapping("DDJ", Control { number: 0, ..PAD }, "mode.padsA.hotcue"), false);
        mappings.learn(mapping("DDJ", roll_key, "mode.padsA.roll"), false);
        mappings.learn(Mapping { when: Some(mode("hotcue")), ..mapping("DDJ", PAD, "deck1.hotcue1") }, false);
        mappings.learn(Mapping { when: Some(mode("roll")), ..mapping("DDJ", PAD, "deck1.roll1") }, false);
        assert_eq!(mappings.all().len(), 4, "one pad, a use in each mode");
        let mut reader = Reader::default();
        reader.set_modes("DDJ", Modes::from([("padsA".into(), "hotcue".into())]));
        assert_eq!(read(&mut reader, &mappings, &[0x90, 36, 127]).unwrap().0, "deck1.hotcue1");
        assert_eq!(read(&mut reader, &mappings, &[0x90, 7, 127]), None, "the mode key is read here");
        assert_eq!(reader.modes("DDJ")["padsA"], "roll");
        // the pad pressed in the other mode ends what it began there
        assert_eq!(read(&mut reader, &mappings, &[0x90, 36, 0]), Some(("deck1.hotcue1".into(), 0.0)));
        assert_eq!(read(&mut reader, &mappings, &[0x90, 36, 127]).unwrap().0, "deck1.roll1");
    }

    #[test]
    fn an_inverted_fader_reads_from_its_top() {
        let mut mappings = Mappings::default();
        mappings.learn(Mapping { invert: true, ..mapping("DDJ", KNOB, "deck1.pitch") }, false);
        let mut reader = Reader::default();
        assert_eq!(read(&mut reader, &mappings, &[0xb0, 20, 127]), Some(("deck1.pitch".into(), 0.0)));
        assert_eq!(read(&mut reader, &mappings, &[0xb0, 20, 0]), Some(("deck1.pitch".into(), 1.0)));
    }

    #[test]
    fn mappings_survive_a_round_trip() {
        let mut mappings = Mappings::default();
        mappings.learn(
            Mapping { encoding: Encoding::RelativeOffset, ..mapping("DDJ", KNOB, "deck1.jog") },
            false,
        );
        mappings.learn(Mapping { fine: Some(32), shift: true, ..mapping("DDJ", PAD, "deck1.pitch") }, false);
        let json = serde_json::to_string(&mappings).unwrap();
        assert_eq!(serde_json::from_str::<Mappings>(&json).unwrap(), mappings);
        // mappings saved before 14-bit controls and layers existed
        let old =
            r#"[{"device":"DDJ","control":{"kind":"note","channel":0,"number":36},"target":"deck1.play"}]"#;
        assert_eq!(serde_json::from_str::<Mappings>(old).unwrap().all(), [mapping("DDJ", PAD, "deck1.play")]);
    }
}
