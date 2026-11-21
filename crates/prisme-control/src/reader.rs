//! What the controllers' events are read as: the layer and the modes they are read in, a
//! 14-bit value's halves, an encoder's steps, a key held, by controller.

use crate::mapping::{Encoding, Mapping, Mappings, Modes, ProfileControl, Shifts};
use crate::message::{Control, Event};
use crate::target::Target;
use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;

/// What an encoder's steps add up to short of a whole step, carried on to its next.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StepCarry(f32);

impl StepCarry {
    /// The whole steps of `steps` and what was carried, what falls short of one carried on.
    pub fn whole(&mut self, steps: f32) -> i32 {
        let total = self.0 + steps;
        let whole = total.trunc();
        self.0 = total - whole;
        whole as i32
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
    /// How many times a shift or a mode key changed the layer read.
    layer_changes: u32,
    pressed: HashMap<Control, (Shifts, Modes)>,
    /// The mode each group held in a mode by a key was in before.
    held_modes: HashMap<Control, (String, String)>,
    /// The groups in a mode for one action.
    for_once: BTreeSet<String>,
    /// The last position of each control read by its changes (`AbsoluteWrap`).
    positions: HashMap<Control, u16>,
    /// What is left of each encoder's scaled steps short of a whole one.
    remainders: HashMap<Control, StepCarry>,
}

/// What is read of a device ([`Reader::seen`]).
pub struct Seen<'r> {
    pub shifts: &'r Shifts,
    pub modes: &'r Modes,
    pressed: &'r HashMap<Control, (Shifts, Modes)>,
}

impl Seen<'_> {
    /// Whether `control` is held down.
    pub fn holds(&self, control: Control) -> bool {
        self.pressed.contains_key(&control)
    }
}

impl Reader {
    /// The action an event of `device` drives and its value; `None` when it drives none, is
    /// a shift or is the first half of a 14-bit value. Shift and the modes are read here: a
    /// mode key's mode is chosen before it is given, but for one waiting to know whether it is
    /// held ([`Reader::choose`]).
    pub fn read<'m>(
        &mut self,
        mappings: &'m Mappings,
        device: &str,
        event: Event,
    ) -> Option<(&'m Mapping, f32)> {
        let reader = self.device(device);
        let key = event.control.is_key();
        let released = key && event.raw == 0;
        let layer = released.then(|| reader.pressed.remove(&event.control)).flatten();
        let mapping = match &layer {
            Some((shifts, modes)) => mappings.find(device, event, shifts, modes),
            None => mappings.find(device, event, &reader.shifts, &reader.modes),
        }?;
        if key && !released {
            reader.pressed.insert(event.control, (reader.shifts.clone(), reader.modes.clone()));
        }
        if let Some(Target::Shift(name)) = mapping.target.target() {
            let held = event.raw > 0;
            let changed = match name {
                Some(name) if held => reader.shifts.names.insert(name.clone()),
                Some(name) => reader.shifts.names.remove(name),
                None => std::mem::replace(&mut reader.shifts.plain, held) != held,
            };
            if changed {
                reader.layer_changes = reader.layer_changes.wrapping_add(1);
            }
            return None;
        }
        if let Some((group, mode)) = mapping.target.mode() {
            if mapping.hold.is_none() {
                reader.select(mapping, event, group, mode);
            }
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
        self.device(device).remainders.entry(control).or_default().whole(steps)
    }

    /// What is read of `device` now: its shifts held, the modes its groups are in, its keys
    /// held.
    pub fn seen(&self, device: &str) -> Seen<'_> {
        static UNREAD: LazyLock<DeviceReader> = LazyLock::new(DeviceReader::default);
        let reader = self.devices.get(device).unwrap_or(&UNREAD);
        Seen { shifts: &reader.shifts, modes: &reader.modes, pressed: &reader.pressed }
    }

    /// How many times `device`'s shifts and mode keys changed the layer it reads: a change
    /// shows in another count.
    pub fn layer_changes(&self, device: &str) -> u32 {
        self.devices.get(device).map_or(0, |d| d.layer_changes)
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

    /// Chooses the mode of `key`, a mode key of `device` pressed and let go: one with a hold,
    /// let go before it.
    pub fn choose(&mut self, device: &str, key: &ProfileControl) {
        let Some((group, mode)) = key.target.mode() else { return };
        let reader = self.device(device);
        for raw in [127, 0] {
            reader.select(key, Event { control: key.control, raw }, group, mode);
        }
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
            self.layer_changes = self.layer_changes.wrapping_add(1);
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
        let range = 1 << event.control.bits();
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
