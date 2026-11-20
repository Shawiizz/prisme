//! What a control's messages drive: the layers and modes read, the keys held, latched, pressed
//! twice or together.

use super::{
    driven, inputs, key_steps, Action, Controllers, DeviceKeys, Holding, KeyWays, Reported, Routed,
    UI_ACTION_EVENT,
};
use crate::actions::{Host, InTurn, Moved, Source};
use crate::dto::UiActionDto;
use crate::state::lock;
use prisme_actions::hold::{Pressed, Timing};
use prisme_actions::input::Input;
use prisme_control::hid::Read;
use prisme_control::{Control, Event, FaderSet, Hold, Mapping, Owner, Reader, TargetId};
use std::time::{Duration, Instant};

/// An action of a key held, and when on its controller's clock.
type Timed = (Action, f64);

impl Controllers {
    /// An event of `device`, whose jog wheels turn by `jog_steps_per_turn`, at `at_s` on its
    /// clock.
    pub(super) fn receive(
        &self,
        host: &impl Host,
        device: &str,
        jog_steps_per_turn: f64,
        event: Event,
        at_s: f64,
    ) {
        let layer = || {
            let reader = lock(&self.reader);
            (reader.mode_changes(device), reader.shifts(device))
        };
        let before = layer();
        let routed = self.route(device, event, at_s);
        let relayered = layer() != before;
        match routed {
            Some(Routed::Learned(_)) => self.show(host.app_state()),
            Some(Routed::Actions(actions)) => {
                self.act(host, actions, Moved { device: device.into(), at_s, jog_steps_per_turn })
            }
            None => {}
        }
        // a key's light may show it held, latched or at the second of its two positions, and
        // what its action did; the lights show the layer in use (a mode chosen by a switch,
        // say)
        if event.control.is_key() || relayered {
            self.show_lights(host, device);
        }
    }

    /// A report of the HID controller `device`, at `at_s` on its clock (see [`Self::read_hid`]).
    pub(super) fn receive_hid(
        &self,
        host: &impl Host,
        device: &str,
        read: &Read,
        jog_steps_per_turn: f64,
        at_s: f64,
    ) {
        match self.read_hid(device, read) {
            Reported::Learnt(_) => self.show(host.app_state()),
            Reported::Events(events) => {
                for event in events {
                    self.receive(host, device, jog_steps_per_turn, event, at_s);
                }
            }
        }
    }

    /// What a report of the HID controller `device` tells: the control learnt, if one is waited
    /// for and the report moved one, else an event for each field read that changed.
    pub(super) fn read_hid(&self, device: &str, read: &Read) -> Reported {
        let fields = lock(&self.hid_fields).get(device).cloned().unwrap_or_default();
        let learning = lock(&self.learning).as_ref().map(|l| l.learnt);
        if let Some(learnt) = learning {
            let mut known = self.profile(device).map(|p| p.hid_inputs()).unwrap_or_default();
            known.extend(fields.iter());
            if let Some(mapping) =
                read.learnt(&known, learnt).and_then(|event| self.learn_from(device, event))
            {
                return Reported::Learnt(Box::new(mapping));
            }
        }
        Reported::Events(read.events(&fields))
    }

    /// Performs what a controller drove (`moved`), in turn: the interface is told the actions
    /// it performs, given what they are given (an encoder's whole steps), the others are done
    /// here.
    pub(super) fn act(&self, host: &impl Host, actions: Vec<Action>, moved: Moved) {
        let in_turn = &mut InTurn::default();
        for action in actions {
            match action {
                Action::Ui(id, input) => {
                    let Some(target) = id.target() else { continue };
                    if let Some(value) = host.actions().interface_value(&moved.device, target, input) {
                        host.tell(UI_ACTION_EVENT, UiActionDto { target: id.into(), value });
                    }
                }
                Action::Rust(target, input) => {
                    host.perform(&target, input, Source::Controller(moved.clone()), in_turn)
                }
            }
        }
    }

    /// Learns the control if a target waits for one, else finds what it drives; `at_s` when,
    /// on the controller's clock.
    pub(super) fn route(&self, device: &str, event: Event, at_s: f64) -> Option<Routed> {
        if let Some(learnt) = self.complete_fine(device, event).or_else(|| self.learn_from(device, event)) {
            return Some(Routed::Learned(Box::new(learnt)));
        }
        let mut reader = lock(&self.reader);
        let mappings = lock(&self.mappings);
        let (mapping, value) = reader.read(&mappings, device, event)?;
        if mapping.target.mode().is_some() {
            return match &mapping.hold {
                Some(hold) => self.hold(device, event, at_s, mapping, hold, &mut reader),
                None => driven(inputs(mapping, value)),
            };
        }
        if let Some(routed) = self.instead(device, event, mapping, &reader) {
            return routed;
        }
        if let Some(down) = &mapping.down {
            let steps = reader.whole_steps(device, mapping.control, value);
            return key_steps(&mapping.target, down, steps);
        }
        if let Some(hold) = &mapping.hold {
            return self.hold(device, event, at_s, mapping, hold, &mut reader);
        }
        driven(inputs(mapping, value))
    }

    /// What a key drives instead of its action as it is: another action pressed twice or
    /// with another key held, a fader put at a position, its action latched. `None` when it
    /// drives its action as it is.
    pub(super) fn instead(
        &self,
        device: &str,
        event: Event,
        mapping: &Mapping,
        reader: &Reader,
    ) -> Option<Option<Routed>> {
        let key = mapping.control;
        let pressed = event.raw > 0;
        let mut keys = lock(&self.keys);
        let keys = keys.of(device);
        let press = |target: &TargetId, value| Some(driven(vec![Action::press(target, value)]));
        if !pressed {
            if let Some(target) = keys.instead.remove(&key) {
                return press(&target, 0.0);
            }
        }
        if pressed {
            if let Some(chord) = mapping.chord.as_ref().filter(|c| reader.seen(device).holds(c.with)) {
                keys.instead.insert(key, chord.target.clone());
                return press(&chord.target, 1.0);
            }
            if let Some(double) = &mapping.double {
                let before = keys.pressed_at.insert(key, Instant::now());
                if before.is_some_and(|at| at.elapsed() < Duration::from_millis(u64::from(double.ms))) {
                    keys.instead.insert(key, double.target.clone());
                    return press(&double.target, 1.0);
                }
            }
        }
        if let Some(set) = mapping.set {
            if !pressed {
                return Some(None);
            }
            let position = match set {
                FaderSet::At(position) => position,
                FaderSet::InTurn([first, second]) if keys.second.remove(&key) => first,
                FaderSet::InTurn([_, second]) => {
                    keys.second.insert(key);
                    second
                }
            };
            return press(&mapping.target, position);
        }
        if mapping.latch {
            if !pressed {
                return Some(None);
            }
            let down = keys.latched.insert(key) || !keys.latched.remove(&key);
            return press(&mapping.target, if down { 1.0 } else { 0.0 });
        }
        None
    }

    /// A key whose action waits to know whether it is held: pressed, nothing yet; let go
    /// before its time, its own actions (those it drives along too) pressed and let go, or its
    /// mode chosen; after, the hold's let go.
    pub(super) fn hold(
        &self,
        device: &str,
        event: Event,
        at_s: f64,
        mapping: &Mapping,
        hold: &Hold,
        reader: &mut Reader,
    ) -> Option<Routed> {
        let mut holdings = lock(&self.holdings);
        if event.raw > 0 {
            holdings.push(Holding {
                device: device.to_owned(),
                control: event.control,
                hold: hold.clone(),
                since: Instant::now(),
                at_s,
                pressed: Pressed::new(Timing::Held(hold.ms), 0.0),
            });
            return None;
        }
        let at = holdings
            .iter()
            .position(|h| h.device == device && h.control == event.control && !h.pressed.released())?;
        if holdings[at].pressed.let_go() {
            // the clock performs its hold now: let go after it (see `hold_fired`)
            holdings[at].at_s = at_s;
            return None;
        }
        let mut held = holdings.remove(at);
        // let go after its time, the clock late: held all the same
        let late = !held.pressed.acted() && held.pressed.due(held.since.elapsed().as_secs_f64());
        let target = &held.hold.target;
        if let Some((group, mode)) = target.mode() {
            if late {
                reader.set_mode(device, group, mode);
            }
            if held.pressed.acted() {
                return None;
            }
        } else if late {
            return driven(vec![Action::press(target, 1.0), Action::press(target, 0.0)]);
        } else if held.pressed.acted() {
            return driven(vec![Action::press(target, 0.0)]);
        }
        reader.choose(device, mapping);
        let mut pressed = inputs(mapping, 1.0);
        pressed.extend(inputs(mapping, 0.0));
        driven(pressed)
    }

    /// The keys held long enough: their hold's action pressed, or its mode chosen; let go after
    /// it, the key let go meanwhile.
    pub fn fire_holds(&self, host: &impl Host) {
        for (device, control, pressed) in self.fired_holds() {
            match pressed {
                Some(pressed) => self.act_held(host, &device, pressed),
                None => self.show_lights(host, &device),
            }
            if let Some(let_go) = self.hold_fired(&device, control) {
                self.act_held(host, &device, let_go);
            }
        }
    }

    /// The action of a key of `device` held, at its time on the controller's clock.
    fn act_held(&self, host: &impl Host, device: &str, (action, at_s): Timed) {
        let jog_steps_per_turn = self.jog_steps_per_turn(device);
        self.act(host, vec![action], Moved { device: device.into(), at_s, jog_steps_per_turn });
    }

    /// The holds whose time came, by controller and key: the action each presses and when, on
    /// the controller's clock, or `None` for a mode it chose (here). Each is said performed
    /// after ([`Self::hold_fired`]).
    pub(super) fn fired_holds(&self) -> Vec<(String, Control, Option<Timed>)> {
        let mut fired = Vec::new();
        for held in lock(&self.holdings).iter_mut() {
            if held.pressed.due(held.since.elapsed().as_secs_f64()) {
                let at_s = held.at_s + f64::from(held.hold.ms) / 1000.0;
                fired.push((held.device.clone(), held.control, held.hold.target.clone(), at_s));
            }
        }
        // the reader once the holdings are let go: a key read takes them the other way round
        fired
            .into_iter()
            .map(|(device, control, target, at_s)| {
                let pressed = match target.mode() {
                    Some((group, mode)) => {
                        lock(&self.reader).set_mode(&device, group, mode);
                        None
                    }
                    None => Some((Action::press(&target, 1.0), at_s)),
                };
                (device, control, pressed)
            })
            .collect()
    }

    /// The hold of `device`'s key `control` performed: its action let go and when, the key let
    /// go meanwhile (a mode chosen has nothing to let go).
    pub(super) fn hold_fired(&self, device: &str, control: Control) -> Option<Timed> {
        let mut holdings = lock(&self.holdings);
        let at = holdings.iter().position(|h| h.device == device && h.control == control)?;
        if !holdings[at].pressed.fired() {
            return None;
        }
        let held = holdings.remove(at);
        let target = held.hold.target;
        target.mode().is_none().then(|| (Action::press(&target, 0.0), held.at_s))
    }
}

impl Action {
    /// `target` driven to `value` (an encoder's `steps`, each moving a fader by `step`),
    /// performed where it is.
    pub(super) fn of(target: &TargetId, value: f32, steps: bool, step: Option<f32>) -> Self {
        let input = Input::of(value, steps, step);
        match target.target() {
            Some(rust) if rust.owner() == Owner::Rust => Action::Rust(rust.clone(), input),
            _ => Action::Ui(target.clone(), input),
        }
    }

    /// The key of `target` pressed to `value`, or let go.
    pub(super) fn press(target: &TargetId, value: f32) -> Self {
        Self::of(target, value, false, None)
    }
}

impl KeyWays {
    /// The keys of `device`: its name is copied once, not at each message.
    pub(super) fn of(&mut self, device: &str) -> &mut DeviceKeys {
        if !self.0.contains_key(device) {
            self.0.insert(device.to_owned(), DeviceKeys::default());
        }
        self.0.get_mut(device).expect("inserted above")
    }

    /// Whether the key `control` of `device` put its fader at the second of its two positions.
    pub(super) fn at_second(&self, device: &str, control: Control) -> bool {
        self.0.get(device).is_some_and(|keys| keys.second.contains(&control))
    }
}
