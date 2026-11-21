//! What the controllers show at each tick of the clock: their lights, the values and levels they
//! show, the lights that blink.

use super::{coloured, light_of, Blink, Connection, Controllers, Levels, Lighting, Lit, Shade, PRESSED};
use crate::actions::Host;
use crate::state::lock;
use prisme_actions::light::light;
use prisme_actions::perform::ClipHold;
use prisme_actions::position::output;
use prisme_actions::view::View;
use prisme_control::{Control, FaderSet, LightValue, Profile, ShownRoom, Target, TargetId, OFF, ON};
use std::time::{Duration, Instant};

impl Controllers {
    /// Shows on `device`'s lights the state of their actions.
    pub(super) fn show_lights(&self, host: &impl Host, device: &str) {
        let view = host.view();
        if let Some(connection) = lock(&self.connections).iter_mut().find(|c| c.device == device) {
            self.show_on(connection, &view);
        }
    }

    /// Shows on `connection`'s lights what `view` shows of their actions (see [`Self::lights`]):
    /// nothing is allocated, the lights changed are sent at once.
    pub(super) fn show_on(&self, connection: &mut Connection, view: &View) {
        let Connection { device, profile, link, lit, room, .. } = connection;
        self.lights(device, profile.as_deref(), view, room, |control, shade| {
            if let Some(lighting) = lit.light(control, shade) {
                link.apply(lighting);
            }
        });
    }

    /// What the lights of `device`, driven by `profile`, show of `view`, each given to `show`,
    /// in the modes it is in and the shifts held: the state of the actions its mapped controls
    /// and its mode keys drive (of a control mapped in several layers, the one in use's), and
    /// of the other lights its profile gives. A key lit while held, whose action has no state of
    /// its own, is off once let go. The mappings shown are worked out in `room`.
    pub(super) fn lights(
        &self,
        device: &str,
        profile: Option<&Profile>,
        view: &View,
        room: &mut ShownRoom,
        mut show: impl FnMut(Control, Shade),
    ) {
        let hid = profile.and_then(|p| p.hid.as_ref());
        // in the order a key read takes them
        let reader = lock(&self.reader);
        let seen = reader.seen(device);
        let mappings = lock(&self.mappings);
        let keys = lock(&self.keys);
        // the state of an action, and the colour it shows
        let shown = |id: &TargetId| match id.target()? {
            Target::Mode { group, mode } => {
                Some((if seen.modes.get(group).is_some_and(|m| m == mode) { ON } else { OFF }, None))
            }
            target => light(target, view).map(|l| (l.state.name(), l.colour)),
        };
        for mapping in mappings.shown(device, seen.shifts, seen.modes, room) {
            let Some(light) = light_of(hid, mapping.control) else { continue };
            let lit_held = mapping.light.has(PRESSED);
            let action = shown(&mapping.target);
            let state = if lit_held && seen.holds(mapping.control) {
                Some(PRESSED)
            } else if matches!(mapping.set, Some(FaderSet::InTurn(_))) {
                Some(if keys.at_second(device, mapping.control) { ON } else { OFF })
            } else {
                action.map(|(state, _)| state).or(lit_held.then_some(OFF))
            };
            if let Some(state) = state {
                let colour = action.and_then(|(_, colour)| colour);
                show(light, coloured(mapping.light.value(state), colour, &mapping.palette, light, hid));
            }
        }
        for extra in profile.iter().flat_map(|p| &p.lights) {
            let action = shown(&extra.target);
            if let Some(value) = action.and_then(|(state, _)| extra.value(state, seen.modes)) {
                let colour = action.and_then(|(_, colour)| colour);
                show(extra.control, coloured(value, colour, &extra.palette, extra.control, hid));
            }
        }
    }

    /// What the lights of `device` show of `view` (see [`Self::lights`]).
    #[cfg(test)]
    pub(super) fn lights_of(&self, device: &str, view: &View) -> Vec<(Control, LightValue)> {
        let mut lights = Vec::new();
        let profile = self.profile(device);
        let mut room = ShownRoom::default();
        self.lights(device, profile.as_deref(), view, &mut room, |control, shade| {
            lights.push((control, shade.value()));
        });
        lights
    }

    /// A tick of the backend's clock (`clock`), the holds whose time came fired before `view` was
    /// built ([`Self::fire_holds`]): what each controller shows of it at `now_s` on the actions'
    /// clock, and of the `levels` of its meters when they are due.
    pub fn tick(&self, view: &View, levels: Option<&Levels>, now_s: f64) {
        for connection in lock(&self.connections).iter_mut() {
            self.show_on(connection, view);
            connection.blink();
            connection.send_due();
            let Some(profile) = connection.profile.clone() else { continue };
            let reader = lock(&self.reader);
            let modes = reader.seen(&connection.device).modes;
            let shown = |when: &Option<prisme_control::When>| {
                when.as_ref().is_none_or(|w| modes.get(&w.group) == Some(&w.mode))
            };
            if let Some(keep) = &profile.keep_alive {
                if connection.kept_alive.elapsed() >= Duration::from_millis(u64::from(keep.every_ms)) {
                    connection.kept_alive = Instant::now();
                    connection.send(&keep.message);
                }
            }
            connection.clipped.resize(profile.meters.len(), ClipHold::default());
            for (at, meter) in profile.meters.iter().enumerate().filter(|(_, m)| shown(&m.when)) {
                let Some(level) = levels.and_then(|l| l.of_source(&meter.source)) else { continue };
                let clipping = connection.clipped[at].heard(level, now_s);
                meter.show(level, clipping, |control, value| connection.light(control, Shade::Given(value)));
            }
            let value_of = |source: &prisme_control::SourceId| output(source.named()?, view);
            for output in profile.outputs.iter().filter(|o| shown(&o.when)) {
                if let Some(value) = value_of(&output.source) {
                    connection.light(output.control, Shade::Steady(output.value(value as f32)));
                }
            }
            connection.displayed.resize(profile.displays.len(), None);
            for (at, display) in profile.displays.iter().enumerate().filter(|(_, d)| shown(&d.when)) {
                let Some(value) = value_of(&display.source) else { continue };
                let number = Some(display.number(value));
                if connection.displayed[at] != number {
                    connection.send(&display.message(value));
                    connection.displayed[at] = number;
                }
            }
        }
    }
}

impl Shade<'_> {
    #[cfg(test)]
    pub(super) fn value(self) -> LightValue {
        match self {
            Shade::Given(value) => value.clone(),
            Shade::Steady(value) => LightValue::Steady(value),
            Shade::Rgb(bytes) => LightValue::Message(bytes.to_vec()),
        }
    }
}

impl Lit {
    /// `control` at `value`, unless it shows it already.
    fn show(&mut self, control: Control, value: u8) -> Option<Lighting<'static>> {
        (self.shown.insert(control, value) != Some(value)).then_some(Lighting::Value(control, value))
    }

    /// The change setting the light of `control` to `shade`, blinking or not, unless it shows
    /// it already.
    pub(super) fn light(&mut self, control: Control, shade: Shade) -> Option<Lighting<'_>> {
        match shade {
            Shade::Given(&LightValue::Steady(value)) | Shade::Steady(value) => {
                self.messages.remove(&control);
                self.blinking.remove(&control);
                self.show(control, value)
            }
            Shade::Given(&LightValue::Blink { blink, ms }) => {
                self.messages.remove(&control);
                if self.blinking.get(&control).is_some_and(|b| b.values == blink && b.ms == ms) {
                    return None;
                }
                self.blinking.insert(control, Blink { values: blink, ms, since: Instant::now() });
                self.show(control, blink[0])
            }
            Shade::Given(LightValue::Message(message)) => self.message(control, message),
            Shade::Rgb(bytes) => self.message(control, &bytes),
        }
    }

    /// `control` shown by a message of its own, unless it shows it already; the message kept
    /// is written over, not made again.
    fn message(&mut self, control: Control, message: &[u8]) -> Option<Lighting<'_>> {
        self.blinking.remove(&control);
        self.shown.remove(&control);
        let kept = self.messages.entry(control).or_default();
        if kept.as_slice() == message {
            return None;
        }
        kept.clear();
        kept.extend_from_slice(message);
        Some(Lighting::Message(control, kept))
    }

    /// Each blinking light at its value of the moment, `apply`ed unless it shows it already.
    pub(super) fn blink(&mut self, mut apply: impl FnMut(Lighting)) {
        for (&control, b) in &self.blinking {
            let half = (b.since.elapsed().as_millis() / u128::from(b.ms.max(1))) % 2;
            let value = b.values[half as usize];
            if self.shown.insert(control, value) != Some(value) {
                apply(Lighting::Value(control, value));
            }
        }
    }
}

impl Levels {
    /// The levels of the engine at `status`.
    pub fn of(status: &prisme_engine::EngineStatus) -> Self {
        let decks = std::array::from_fn(|d| status.decks[d].peak);
        Levels { decks, deck_count: status.deck_count, main: status.main_peak, sides: status.master_peaks }
    }

    /// The level of a meter's `source`: `deck1`… (one the engine runs), `main`, `main.left`,
    /// `main.right`.
    pub(super) fn of_source(&self, source: &str) -> Option<f32> {
        match source {
            "main" => Some(self.main),
            "main.left" => Some(self.sides[0]),
            "main.right" => Some(self.sides[1]),
            _ => {
                let deck: usize = source.strip_prefix("deck")?.parse().ok()?;
                (1..=self.deck_count).contains(&deck).then(|| self.decks[deck - 1])
            }
        }
    }
}
