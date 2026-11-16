use crate::light::LightValues;
use crate::message::{Control, ControlKind, Event};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// The action that turns a controller's other controls into their second layer while held;
/// `shift.<name>` a named shift's (one per side, say), that is a shift too.
pub const SHIFT: &str = "shift";

/// The actions that put a controller's group of controls in a mode: `mode.<group>.<mode>`.
pub const MODE: &str = "mode.";

/// The mode of each group of a controller's controls (the page of a side's pads, say).
pub type Modes = HashMap<String, String>;

/// The group and the mode an action selects, if it is a mode's.
pub fn selected_mode(target: &str) -> Option<(&str, &str)> {
    target.strip_prefix(MODE)?.split_once('.')
}

/// A second action for a key held down for `ms` milliseconds: pressed and let go sooner, it
/// drives its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Hold {
    pub ms: u32,
    pub target: String,
}

/// Another action for a key pressed while the key `with` is held.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Chord {
    pub with: Control,
    pub target: String,
}

/// Where a key puts a fader's action: at a position, or at each of two in turn.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum FaderSet {
    At(f32),
    InTurn([f32; 2]),
}

/// The layer of a control: the first, the second while any shift is held (`true`), or a named
/// shift's while that one is held (`"left"`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(from = "ShiftWritten", into = "ShiftWritten")]
pub enum Shift {
    #[default]
    None,
    Any,
    Named(String),
}

#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
enum ShiftWritten {
    Held(bool),
    Named(String),
}

impl From<ShiftWritten> for Shift {
    fn from(written: ShiftWritten) -> Self {
        match written {
            ShiftWritten::Held(false) => Shift::None,
            ShiftWritten::Held(true) => Shift::Any,
            ShiftWritten::Named(name) => Shift::Named(name),
        }
    }
}

impl From<Shift> for ShiftWritten {
    fn from(shift: Shift) -> Self {
        match shift {
            Shift::None => ShiftWritten::Held(false),
            Shift::Any => ShiftWritten::Held(true),
            Shift::Named(name) => ShiftWritten::Named(name),
        }
    }
}

impl From<bool> for Shift {
    fn from(held: bool) -> Self {
        if held {
            Shift::Any
        } else {
            Shift::None
        }
    }
}

impl Shift {
    pub fn is_none(&self) -> bool {
        *self == Shift::None
    }
}

/// The shifts of a controller held: its shift, and its named shifts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shifts {
    plain: bool,
    names: BTreeSet<String>,
}

impl Shifts {
    /// Whether any shift is held.
    pub fn any(&self) -> bool {
        self.plain || !self.names.is_empty()
    }
}

/// A mode of a controller: `mode` of its group `group`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct When {
    pub group: String,
    pub mode: String,
}

/// How a control's values are read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
    /// Steps from zero: 0 is one step to the right, 127 one to the left (Denon).
    RelativeZero,
    /// A position that wraps around (a jog wheel's angle): its changes are the steps.
    AbsoluteWrap,
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
            (_, Encoding::RelativeZero) if raw < 64 => steps(i32::from(raw) + 1),
            (_, Encoding::RelativeZero) => steps(i32::from(raw) - 128),
            // the steps come from the position before, which the reader keeps
            (_, Encoding::AbsoluteWrap) => 0.0,
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

/// A control and the action it drives, and how: a control of a profile, or one learnt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileControl {
    pub control: Control,
    pub target: String,
    #[serde(default, skip_serializing_if = "is_absolute")]
    pub encoding: Encoding,
    /// The controller sending the low 7 bits of a 14-bit control (its high 7 bits on
    /// `control`, by convention the number 32 lower).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fine: Option<u8>,
    /// Drives its action while the controller's shift is held (its second layer), or a
    /// named shift.
    #[serde(default, skip_serializing_if = "Shift::is_none")]
    pub shift: Shift,
    /// Drives its action only while its group is in this mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
    /// A fader whose lowest value is at its top.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub invert: bool,
    /// What its light shows in each state of its action, when it is not 127 and 0.
    #[serde(default, skip_serializing_if = "LightValues::is_empty")]
    pub light: LightValues,
    /// Drives its action, as a key pressed, when the control sends this value (a switch in
    /// one of its positions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_value: Option<u16>,
    /// Other actions it drives along with its own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also: Vec<String>,
    /// Another action for the key held down a while.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold: Option<Hold>,
    /// Another action for the key pressed twice within `ms`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub double: Option<Hold>,
    /// Another action for the key pressed while another is held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chord: Option<Chord>,
    /// An encoder driving a key's action: a step to the right presses `target`, a step to the
    /// left this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub down: Option<String>,
    /// An encoder's steps multiplied by this; below 1 they add up to whole steps.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub scale: f32,
    /// How far a step of an encoder moves a fader's action, a share of its travel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f32>,
    /// A key putting a fader's action at a position, or at each of two in turn (lit at the
    /// second).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set: Option<FaderSet>,
    /// A key driving an action while held, latched: a press holds it, the next lets it go.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub latch: bool,
    /// A mode key choosing its mode while held: let go, its group is back in the mode before.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub held: bool,
    /// A mode key choosing its mode for one action of its group, which then goes back to its
    /// first mode.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub once: bool,
    /// Its light's values by the colour of its action (a hot cue's, `"#ff0000": 5`): the
    /// nearest colour's.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub palette: BTreeMap<String, u8>,
}

fn is_absolute(encoding: &Encoding) -> bool {
    *encoding == Encoding::Absolute
}

fn one() -> f32 {
    1.0
}

fn is_one(value: &f32) -> bool {
    *value == 1.0
}

impl Default for ProfileControl {
    fn default() -> Self {
        Self {
            control: Control::default(),
            target: String::new(),
            encoding: Encoding::Absolute,
            fine: None,
            shift: Shift::None,
            when: None,
            invert: false,
            light: LightValues::default(),
            on_value: None,
            also: Vec::new(),
            hold: None,
            double: None,
            chord: None,
            down: None,
            scale: 1.0,
            step: None,
            set: None,
            latch: false,
            held: false,
            once: false,
            palette: BTreeMap::new(),
        }
    }
}

impl ProfileControl {
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

    /// Whether `event` is one of its own: a switch's mapping takes its position only.
    fn takes(&self, event: Event) -> bool {
        self.sent_by(event.control) && self.on_value.is_none_or(|v| v == event.raw)
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

/// A control of a controller driving one of the application's actions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mapping {
    /// The controller's MIDI port name.
    pub device: String,
    #[serde(flatten)]
    pub binding: ProfileControl,
}

impl Mapping {
    pub fn new(device: &str, binding: ProfileControl) -> Self {
        Self { device: device.to_owned(), binding }
    }
}

impl std::ops::Deref for Mapping {
    type Target = ProfileControl;

    fn deref(&self) -> &ProfileControl {
        &self.binding
    }
}

impl std::ops::DerefMut for Mapping {
    fn deref_mut(&mut self) -> &mut ProfileControl {
        &mut self.binding
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

    /// The mapping `event` of `device` belongs to in `modes`, `shifts` held: in a named shift's
    /// layer, else in the shift layer while a shift is held, else in the first layer (a
    /// control without a second use keeps its first).
    pub fn find(&self, device: &str, event: Event, shifts: &Shifts, modes: &Modes) -> Option<&Mapping> {
        let find = |layer: &dyn Fn(&Shift) -> bool| {
            self.0
                .iter()
                .find(|m| m.device == device && layer(&m.shift) && m.applies(modes) && m.takes(event))
        };
        let named = shifts.names.iter().find_map(|name| find(&|s| matches!(s, Shift::Named(n) if n == name)));
        named
            .or_else(|| find(&|s| *s == Shift::Any).filter(|_| shifts.any()))
            .or_else(|| find(&|s| *s == Shift::None))
    }

    /// Adds `mapping`, replacing what its control did in its layer and the control its
    /// action had on that controller, unless the action takes `several`.
    pub fn learn(&mut self, mapping: Mapping, several: bool) {
        self.0.retain(|m| {
            let same_control = m.control == mapping.control
                && m.shift == mapping.shift
                && m.when == mapping.when
                && m.on_value == mapping.on_value;
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
    shifts: Shifts,
    modes: Modes,
    /// The modes its groups begin in.
    first_modes: Modes,
    /// How many times a mode key changed a mode.
    mode_changes: u32,
    pressed: HashMap<Control, (Shifts, Modes)>,
    /// The mode each group held in a mode by a key was in before.
    held_modes: HashMap<Control, (String, String)>,
    /// The groups in a mode for one action.
    for_once: BTreeSet<String>,
    /// The last position of each control read by its changes (`AbsoluteWrap`).
    positions: HashMap<Control, u16>,
    /// What is left of each encoder's scaled steps short of a whole one.
    remainders: HashMap<Control, f32>,
}

impl Reader {
    /// The action an event of `device` drives and its value; `None` when it drives none, is
    /// a shift or is the first half of a 14-bit value. Shift and the modes are read here: a
    /// mode key's mode is chosen before it is given.
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
            Some((shifts, modes)) => mappings.find(device, event, shifts, modes),
            None => mappings.find(device, event, &reader.shifts, &reader.modes),
        }?;
        if key && !released {
            reader.pressed.insert(event.control, (reader.shifts.clone(), reader.modes.clone()));
        }
        if let Some(name) = mapping.target.strip_prefix(SHIFT) {
            let held = event.raw > 0;
            match name.strip_prefix('.') {
                Some(name) if held => drop(reader.shifts.names.insert(name.to_owned())),
                Some(name) => drop(reader.shifts.names.remove(name)),
                None => reader.shifts.plain = held,
            }
            return None;
        }
        if let Some((group, mode)) = selected_mode(&mapping.target) {
            reader.select(mapping, event, group, mode);
            let value = if mapping.on_value.is_some() { 1.0 } else { mapping.encoding.value(event) };
            return Some((mapping, value));
        }
        if !released {
            if let Some(when) = &mapping.when {
                if reader.for_once.remove(&when.group) {
                    reader.back_to_first(&when.group);
                }
            }
        }
        if mapping.encoding == Encoding::AbsoluteWrap {
            return reader.wrapped(mapping, event).map(|steps| (mapping, mapping.scaled(steps)));
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
            None if mapping.on_value.is_some() => Some((mapping, 1.0)),
            None if mapping.encoding.is_relative() => {
                Some((mapping, mapping.scaled(mapping.encoding.value(event))))
            }
            None => Some((mapping, mapping.position(mapping.encoding.value(event)))),
        }
    }

    /// Whether a shift of `device` is held.
    pub fn shifted(&self, device: &str) -> bool {
        self.devices.get(device).is_some_and(|d| d.shifts.any())
    }

    /// The whole steps of `steps` of `control`, what falls short of one kept for the next.
    pub fn whole_steps(&mut self, device: &str, control: Control, steps: f32) -> i32 {
        let remainder = self.device(device).remainders.entry(control).or_default();
        let total = *remainder + steps;
        let whole = total.trunc();
        *remainder = total - whole;
        whole as i32
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
        let reader = self.device(device);
        reader.first_modes = modes.clone();
        reader.modes = modes;
    }

    /// Puts `device`'s `group` in `mode` (a key held long enough, say).
    pub fn set_mode(&mut self, device: &str, group: &str, mode: &str) {
        self.device(device).change_mode(group, mode);
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

impl DeviceReader {
    /// A mode key pressed or let go: its mode chosen, for good, while held, or for one action.
    fn select(&mut self, key: &ProfileControl, event: Event, group: &str, mode: &str) {
        let pressed = event.raw > 0 || key.on_value.is_some();
        if key.held && !pressed {
            if let Some((group, before)) = self.held_modes.remove(&key.control) {
                self.change_mode(&group, &before);
            }
            return;
        }
        if !pressed {
            return;
        }
        if key.held {
            let before = self.modes.get(group).cloned().unwrap_or_default();
            self.held_modes.insert(key.control, (group.to_owned(), before));
        }
        if key.once {
            self.for_once.insert(group.to_owned());
        }
        self.change_mode(group, mode);
    }

    fn change_mode(&mut self, group: &str, mode: &str) {
        if self.modes.get(group).is_none_or(|m| m != mode) {
            self.modes.insert(group.to_owned(), mode.to_owned());
            self.mode_changes = self.mode_changes.wrapping_add(1);
        }
    }

    fn back_to_first(&mut self, group: &str) {
        if let Some(first) = self.first_modes.get(group).cloned() {
            self.change_mode(group, &first);
        }
    }

    /// The steps of a control read by its changes of position, the shortest way round; none
    /// on its first position.
    fn wrapped(&mut self, mapping: &ProfileControl, event: Event) -> Option<f32> {
        let range = if event.control.kind == ControlKind::PitchBend { 16384 } else { 128 };
        let before = self.positions.insert(mapping.control, event.raw)?;
        let change = (i32::from(event.raw) - i32::from(before)).rem_euclid(range);
        let steps = if change > range / 2 { change - range } else { change };
        (steps != 0).then_some(steps as f32)
    }
}

impl ProfileControl {
    /// An encoder's steps as they drive its action: multiplied by its scale, the other way
    /// for an inverted one.
    fn scaled(&self, steps: f32) -> f32 {
        let steps = steps * self.scale;
        if self.invert {
            -steps
        } else {
            steps
        }
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
        Mapping::new(device, ProfileControl { control, target: target.into(), ..Default::default() })
    }

    /// `mapping` changed by `change`.
    fn with(mut mapping: Mapping, change: impl FnOnce(&mut ProfileControl)) -> Mapping {
        change(&mut mapping.binding);
        mapping
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
        mappings.learn(with(mapping("DDJ", PAD, "deck1.hotcue1.delete"), |b| b.shift = Shift::Any), false);
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
        mappings.learn(with(mapping("DDJ", PAD, "deck1.censor"), |b| b.shift = Shift::Any), false);
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
        mappings.learn(with(mapping("DDJ", KNOB, "deck1.filter"), |b| b.shift = Shift::Any), false);
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
        let target = |control| {
            let pressed = Event { control, raw: 127 };
            layered.find("DDJ", pressed, &Shifts::default(), &Modes::new()).map(|m| m.target.as_str())
        };
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
        mappings.learn(with(mapping("DDJ", PAD, "deck1.hotcue1"), |b| b.when = Some(mode("hotcue"))), false);
        mappings.learn(with(mapping("DDJ", PAD, "deck1.roll1"), |b| b.when = Some(mode("roll"))), false);
        assert_eq!(mappings.all().len(), 4, "one pad, a use in each mode");
        let mut reader = Reader::default();
        reader.set_modes("DDJ", Modes::from([("padsA".into(), "hotcue".into())]));
        assert_eq!(read(&mut reader, &mappings, &[0x90, 36, 127]).unwrap().0, "deck1.hotcue1");
        assert_eq!(read(&mut reader, &mappings, &[0x90, 7, 127]), Some(("mode.padsA.roll".into(), 1.0)));
        assert_eq!(reader.modes("DDJ")["padsA"], "roll", "the mode key is read here");
        // the pad pressed in the other mode ends what it began there
        assert_eq!(read(&mut reader, &mappings, &[0x90, 36, 0]), Some(("deck1.hotcue1".into(), 0.0)));
        assert_eq!(read(&mut reader, &mappings, &[0x90, 36, 127]).unwrap().0, "deck1.roll1");
    }

    #[test]
    fn a_switch_drives_the_action_of_its_position() {
        let switch = Control { number: 3, ..KNOB };
        // as a profile gives them: two positions may drive the same action
        let positions = [(0, "deck1.fx.off"), (1, "deck1.fx.on"), (2, "deck1.fx.on")]
            .map(|(value, target)| with(mapping("DDJ", switch, target), |b| b.on_value = Some(value)));
        let mappings = Mappings::layered(positions, &Mappings::default(), &[], |_| false);
        assert_eq!(mappings.all().len(), 3, "a position each");
        let mut reader = Reader::default();
        assert_eq!(read(&mut reader, &mappings, &[0xb0, 3, 1]), Some(("deck1.fx.on".into(), 1.0)));
        assert_eq!(read(&mut reader, &mappings, &[0xb0, 3, 0]), Some(("deck1.fx.off".into(), 1.0)));
        assert_eq!(read(&mut reader, &mappings, &[0xb0, 3, 5]), None);
    }

    #[test]
    fn an_inverted_fader_reads_from_its_top() {
        let mut mappings = Mappings::default();
        mappings.learn(with(mapping("DDJ", KNOB, "deck1.pitch"), |b| b.invert = true), false);
        let mut reader = Reader::default();
        assert_eq!(read(&mut reader, &mappings, &[0xb0, 20, 127]), Some(("deck1.pitch".into(), 0.0)));
        assert_eq!(read(&mut reader, &mappings, &[0xb0, 20, 0]), Some(("deck1.pitch".into(), 1.0)));
    }

    #[test]
    fn mappings_survive_a_round_trip() {
        let mut mappings = Mappings::default();
        mappings
            .learn(with(mapping("DDJ", KNOB, "deck1.jog"), |b| b.encoding = Encoding::RelativeOffset), false);
        mappings.learn(
            with(mapping("DDJ", PAD, "deck1.pitch"), |b| {
                b.fine = Some(32);
                b.shift = Shift::Any
            }),
            false,
        );
        let json = serde_json::to_string(&mappings).unwrap();
        assert_eq!(serde_json::from_str::<Mappings>(&json).unwrap(), mappings);
        // mappings saved before 14-bit controls and layers existed
        let old =
            r#"[{"device":"DDJ","control":{"kind":"note","channel":0,"number":36},"target":"deck1.play"}]"#;
        assert_eq!(serde_json::from_str::<Mappings>(old).unwrap().all(), [mapping("DDJ", PAD, "deck1.play")]);
    }

    #[test]
    fn named_shifts_choose_their_own_layer() {
        let left = Control { number: 1, ..SHIFT_KEY };
        let mut mappings = Mappings::default();
        mappings.learn(mapping("DDJ", SHIFT_KEY, SHIFT), false);
        mappings.learn(mapping("DDJ", left, "shift.left"), false);
        mappings.learn(mapping("DDJ", PAD, "deck1.hotcue1"), false);
        mappings.learn(with(mapping("DDJ", PAD, "deck1.hotcue1.delete"), |b| b.shift = Shift::Any), false);
        mappings.learn(
            with(mapping("DDJ", PAD, "deck1.roll1"), |b| b.shift = Shift::Named("left".into())),
            false,
        );
        let mut reader = Reader::default();
        assert_eq!(read(&mut reader, &mappings, &[0x90, 1, 127]), None);
        assert!(reader.shifted("DDJ"));
        assert_eq!(read(&mut reader, &mappings, &[0x90, 36, 127]).unwrap().0, "deck1.roll1");
        assert_eq!(read(&mut reader, &mappings, &[0x90, 36, 0]).unwrap().0, "deck1.roll1");
        read(&mut reader, &mappings, &[0x90, 1, 0]);
        read(&mut reader, &mappings, &[0x90, 63, 127]);
        assert_eq!(read(&mut reader, &mappings, &[0x90, 36, 127]).unwrap().0, "deck1.hotcue1.delete");
    }

    #[test]
    fn a_shift_is_written_as_a_flag_or_a_name() {
        let read = |json: &str| serde_json::from_str::<Shift>(json).unwrap();
        assert_eq!(read("false"), Shift::None);
        assert_eq!(read("true"), Shift::Any);
        assert_eq!(read("\"left\""), Shift::Named("left".into()));
        assert_eq!(serde_json::to_string(&Shift::Named("right".into())).unwrap(), "\"right\"");
        assert_eq!(serde_json::to_string(&Shift::Any).unwrap(), "true");
    }

    #[test]
    fn a_mode_key_held_gives_its_mode_back_when_let_go() {
        let key = Control { number: 9, ..PAD };
        let mut mappings = Mappings::default();
        mappings.learn(with(mapping("DDJ", key, "mode.pads.params"), |b| b.held = true), false);
        let mut reader = Reader::default();
        reader.set_modes("DDJ", Modes::from([("pads".into(), "cues".into())]));
        read(&mut reader, &mappings, &[0x90, 9, 127]);
        assert_eq!(reader.modes("DDJ")["pads"], "params");
        read(&mut reader, &mappings, &[0x90, 9, 0]);
        assert_eq!(reader.modes("DDJ")["pads"], "cues");
    }

    #[test]
    fn a_mode_for_once_ends_with_the_action_of_its_group() {
        let key = Control { number: 9, ..PAD };
        let mut mappings = Mappings::default();
        mappings.learn(with(mapping("DDJ", key, "mode.pads.delete"), |b| b.once = true), false);
        let when = |mode: &str| Some(When { group: "pads".into(), mode: mode.into() });
        mappings.learn(with(mapping("DDJ", PAD, "deck1.hotcue1"), |b| b.when = when("cues")), false);
        mappings.learn(with(mapping("DDJ", PAD, "deck1.hotcue1.delete"), |b| b.when = when("delete")), false);
        let mut reader = Reader::default();
        reader.set_modes("DDJ", Modes::from([("pads".into(), "cues".into())]));
        read(&mut reader, &mappings, &[0x90, 9, 127]);
        assert_eq!(read(&mut reader, &mappings, &[0x90, 36, 127]).unwrap().0, "deck1.hotcue1.delete");
        assert_eq!(reader.modes("DDJ")["pads"], "cues");
        assert_eq!(read(&mut reader, &mappings, &[0x90, 36, 0]).unwrap().0, "deck1.hotcue1.delete");
        assert_eq!(read(&mut reader, &mappings, &[0x90, 36, 127]).unwrap().0, "deck1.hotcue1");
    }

    #[test]
    fn a_switch_position_of_zero_chooses_a_mode() {
        let switch = Control { number: 3, ..KNOB };
        let mut mappings = Mappings::default();
        mappings.learn(with(mapping("DDJ", switch, "mode.decks.left"), |b| b.on_value = Some(0)), false);
        mappings.learn(with(mapping("DDJ", switch, "mode.decks.right"), |b| b.on_value = Some(127)), false);
        let mut reader = Reader::default();
        read(&mut reader, &mappings, &[0xb0, 3, 127]);
        assert_eq!(reader.modes("DDJ")["decks"], "right");
        read(&mut reader, &mappings, &[0xb0, 3, 0]);
        assert_eq!(reader.modes("DDJ")["decks"], "left");
    }

    #[test]
    fn encoders_from_zero_and_wrapping_positions_give_steps() {
        let at = |raw| Event { control: KNOB, raw };
        assert_eq!(Encoding::RelativeZero.value(at(0)), 1.0);
        assert_eq!(Encoding::RelativeZero.value(at(2)), 3.0);
        assert_eq!(Encoding::RelativeZero.value(at(127)), -1.0);
        let mut mappings = Mappings::default();
        mappings
            .learn(with(mapping("DDJ", KNOB, "deck1.jog"), |b| b.encoding = Encoding::AbsoluteWrap), false);
        let mut reader = Reader::default();
        assert_eq!(read(&mut reader, &mappings, &[0xb0, 20, 120]), None, "a first position is no step");
        assert_eq!(read(&mut reader, &mappings, &[0xb0, 20, 124]), Some(("deck1.jog".into(), 4.0)));
        assert_eq!(read(&mut reader, &mappings, &[0xb0, 20, 2]), Some(("deck1.jog".into(), 6.0)), "round");
        assert_eq!(read(&mut reader, &mappings, &[0xb0, 20, 126]), Some(("deck1.jog".into(), -4.0)));
    }

    #[test]
    fn steps_are_scaled_inverted_and_added_up_to_whole_ones() {
        let mut mappings = Mappings::default();
        let browse = |b: &mut ProfileControl| {
            b.encoding = Encoding::RelativeOffset;
            b.scale = 0.25;
            b.invert = true;
        };
        mappings.learn(with(mapping("DDJ", KNOB, "library.browse"), browse), false);
        let mut reader = Reader::default();
        assert_eq!(read(&mut reader, &mappings, &[0xb0, 20, 66]), Some(("library.browse".into(), -0.5)));
        assert_eq!(reader.whole_steps("DDJ", KNOB, -0.5), 0);
        assert_eq!(reader.whole_steps("DDJ", KNOB, -0.75), -1);
        assert_eq!(reader.whole_steps("DDJ", KNOB, 0.25), 0, "what is left counts");
    }

    #[test]
    fn a_fader_set_is_one_position_or_two_in_turn() {
        assert_eq!(serde_json::from_str::<FaderSet>("0.5").unwrap(), FaderSet::At(0.5));
        assert_eq!(serde_json::from_str::<FaderSet>("[0, 1]").unwrap(), FaderSet::InTurn([0.0, 1.0]));
    }
}
