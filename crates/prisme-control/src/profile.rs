use crate::hid::{self, Field, HidDevice};
use crate::id::{SourceId, TargetId};
use crate::light::{LightValue, LightValues, OFF, ON};
use crate::mapping::{Mapping, Modes, ProfileControl, Shift, When};
use crate::message::Control;
use crate::target::Target;
use schemars::JsonSchema;
use serde::{de, Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The version of the profile format, raised when profiles may use what an older Prisme does
/// not read: a profile published after it was raised is left to the Prisme that reads it.
pub const PROFILE_FORMAT: u32 = 4;

/// Steps in a turn of a jog wheel whose model does not tell. Most controllers send
/// hundreds; the number only scales scratches and nudges.
pub const JOG_STEPS_PER_TURN: f64 = 720.0;

/// What Prisme knows of a controller model before anything is learnt on it: the controls
/// it maps, the lights beyond those controls' own, and what it is sent once opened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// The schema of the format, for editors: [`crate::SCHEMA_URL`].
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// The model, as the settings show it.
    pub name: String,
    /// The controller is the MIDI port whose name holds one of these (case aside); of
    /// several models, the one whose name is the longest (a DDJ-SB2 is no DDJ-SB). None for a
    /// HID controller.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<String>,
    /// A HID controller: how it is found, and its output reports. Its controls and lights are
    /// fields of its reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hid: Option<HidDevice>,
    /// Steps in a turn of its jog wheels, measured on the controller.
    #[serde(default = "jog_steps_per_turn", skip_serializing_if = "is_jog_steps_per_turn")]
    pub jog_steps_per_turn: f64,
    /// Where the mapping comes from, when it is not Prisme's own.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub credits: String,
    /// Tried on the controller itself, not only read from another program's mapping.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub verified: bool,
    /// Another profile for the ports of a model (a firmware or an edition of its own): never
    /// found by itself, offered in the settings for a controller its ports fit.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub variant: bool,
    /// Sent when the controller is opened (a request for its knobs' positions, say), some a
    /// while after; to a HID controller, whole output reports.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub init: Vec<InitMessage>,
    /// Sent when it is closed, after its lights went off.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub close: Vec<Vec<u8>>,
    /// Sent again and again while it is open: some models stop answering without it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<KeepAlive>,
    /// Groups of its controls that drive other actions in each of their modes (the pad
    /// pages of a side, chosen by its mode keys), when the controls send the same in each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modes: Vec<ModeGroup>,
    pub controls: Vec<ProfileControl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lights: Vec<ProfileLight>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meters: Vec<Meter>,
    /// Values shown on the controller: a ring around a jog wheel, a knob's ring of lights.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<Output>,
    /// Numbers shown on the controller's screens: the tempo, the time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub displays: Vec<Display>,
}

/// A message sent when a controller is opened: at once, or `after_ms` later (once a light
/// show it plays by itself is over, say).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum InitMessage {
    Now(Vec<u8>),
    Later(LaterMessage),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LaterMessage {
    pub after_ms: u32,
    pub message: Vec<u8>,
}

impl InitMessage {
    /// When it goes after the controller is opened (ms), and what.
    pub fn when_and_what(&self) -> (u32, &[u8]) {
        match self {
            InitMessage::Now(message) => (0, message),
            InitMessage::Later(later) => (later.after_ms, &later.message),
        }
    }
}

/// A value shown on `control`: `source`'s position from 0 to 1 (`deck1.position` of its
/// track, `deck1.turn` of its record at 33⅓, or a fader's: `deck1.fx.mix`), from `min` to
/// `max`; nothing to show (no track, a stem not separated…), 0 if `min` and `max` are above
/// it, else `min`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    pub source: SourceId,
    pub control: Control,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub min: u8,
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub max: u8,
    /// While its group is in this mode only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
}

impl Output {
    /// The value showing `position` (0 to 1).
    pub fn value(&self, position: f32) -> u8 {
        let span = f32::from(self.max) - f32::from(self.min);
        (f32::from(self.min) + position.clamp(0.0, 1.0) * span).round() as u8
    }

    /// The value showing nothing: 0 below values from 1 up, which the controllers keep dark
    /// there (a ring of 1 to 15 lights its first LED at 1, none at 0), else the start of its
    /// travel.
    pub fn off(&self) -> u8 {
        if self.min.min(self.max) > 0 {
            0
        } else {
            self.min
        }
    }
}

/// A number shown on a screen by a message of the controller's own: `source`'s value
/// (`deck1.bpm`, `deck1.elapsed`, `deck1.remaining` and `deck1.duration` in seconds,
/// `deck1.tempo` and `deck1.tempoRange` in percent) times `scale`, plus `offset`, written
/// where the message says `"value"`: in `nibbles` bytes of four bits, the most significant
/// first and a number below zero in two's complement, or without them in one byte from 0 to
/// 127. `sign` puts its first nibble for a number at or above zero, or below it, in place of
/// the first of those.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Display {
    pub source: SourceId,
    pub message: Vec<DisplayPart>,
    #[serde(default = "unscaled", skip_serializing_if = "is_unscaled")]
    pub scale: f64,
    #[serde(default, skip_serializing_if = "is_no_offset")]
    pub offset: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 16))]
    pub nibbles: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sign: Option<[u8; 2]>,
    /// While its group is in this mode only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
}

/// A byte of a display's message, or the place of its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum DisplayPart {
    Byte(u8),
    Value(ValuePlace),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ValuePlace {
    Value,
}

impl Display {
    /// The number it shows for `value`, its source's: its message changes with it alone.
    pub fn number(&self, value: f64) -> i64 {
        (value * self.scale + self.offset).round() as i64
    }

    /// The message showing `value`, its source's.
    pub fn message(&self, value: f64) -> Vec<u8> {
        let number = self.number(value);
        let mut message = Vec::with_capacity(self.message.len() + 16);
        for part in &self.message {
            match (part, self.nibbles) {
                (DisplayPart::Byte(byte), _) => message.push(*byte),
                (DisplayPart::Value(_), None) => message.push(number.clamp(0, 127) as u8),
                (DisplayPart::Value(_), Some(nibbles)) => {
                    let first = message.len();
                    message.extend((0..nibbles.min(16)).rev().map(|at| ((number >> (4 * at)) & 0xF) as u8));
                    if let (Some([at_or_above, below]), Some(nibble)) = (self.sign, message.get_mut(first)) {
                        *nibble = if number < 0 { below } else { at_or_above };
                    }
                }
            }
        }
        message
    }
}

/// A message sent every `every_ms` milliseconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KeepAlive {
    pub every_ms: u32,
    pub message: Vec<u8>,
}

/// A group of controls and its modes, the first one to begin with, each chosen by a key
/// whose light shows whether it is the one (`on`, `off`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModeGroup {
    pub group: String,
    pub modes: Vec<String>,
    pub select: Vec<ModeSelect>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModeSelect {
    pub control: Control,
    pub mode: String,
    /// Chosen while the controller's shift is held, or a named shift.
    #[serde(default, skip_serializing_if = "Shift::is_none")]
    pub shift: Shift,
    #[serde(default, skip_serializing_if = "LightValues::is_empty")]
    pub light: LightValues,
    /// Its mode only while the key is held: let go, the group is back in the mode before.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub held: bool,
    /// Its mode for one action of the group, which then goes back to its first mode.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub once: bool,
}

/// A level shown on the controller: `source`'s (`deck1`…, `main`, `main.left`,
/// `main.right`) raised to `curve` (1 straight), by the value of one control or on LEDs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Meter {
    pub source: String,
    #[serde(flatten)]
    pub on: MeterOn,
    #[serde(default = "straight", skip_serializing_if = "is_straight", deserialize_with = "curve")]
    #[schemars(extend("exclusiveMinimum" = 0))]
    pub curve: f32,
    /// While its group is in this mode only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
}

/// Where a meter shows its level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum MeterOn {
    /// On `control`, from `min` to `max`, or as one of `steps` (the values at silence, then
    /// at each equal share of the level); `clip` for a while after the level reached full
    /// scale.
    Control {
        control: Control,
        #[serde(default, skip_serializing_if = "is_zero")]
        min: u8,
        #[serde(default = "full", skip_serializing_if = "is_full")]
        max: u8,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        steps: Vec<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        clip: Option<u8>,
    },
    /// On LEDs switched one by one, each lit from its own share of the level.
    Leds { leds: Vec<MeterLed> },
}

/// An LED of a meter, lit once the level reaches `from`: its values `on` and `off` beside
/// it, as a light's, and `clip` if it shows another for a while after the level reached full
/// scale.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MeterLed {
    pub control: Control,
    #[schemars(range(min = 0, max = 1))]
    pub from: f32,
    #[serde(flatten)]
    pub values: LightValues,
}

/// The state of an LED of a meter for a while after the level reached full scale.
const CLIP: &str = "clip";

impl Meter {
    /// What its controls show at `level` (full scale 1), `clipping` for a while after the
    /// level reached full scale: each given to `show`, in turn.
    pub fn show(&self, level: f32, clipping: bool, mut show: impl FnMut(Control, &LightValue)) {
        let shaped = level.clamp(0.0, 1.0).powf(self.curve);
        match &self.on {
            MeterOn::Control { control, clip: Some(clip), .. } if clipping => {
                show(*control, &LightValue::Steady(*clip))
            }
            MeterOn::Control { control, min, max, steps, .. } => {
                let value = match steps.len().checked_sub(1) {
                    Some(last) => steps[(shaped * last as f32).round() as usize],
                    None => (f32::from(*min) + shaped * (f32::from(*max) - f32::from(*min))).round() as u8,
                };
                show(*control, &LightValue::Steady(value))
            }
            MeterOn::Leds { leds } => {
                leds.iter().for_each(|led| show(led.control, led.value(shaped, clipping)))
            }
        }
    }
}

impl MeterLed {
    /// Its value at `shaped`, the level past its meter's curve.
    fn value(&self, shaped: f32, clipping: bool) -> &LightValue {
        let state = if clipping && self.values.has(CLIP) {
            CLIP
        } else if shaped >= self.from {
            ON
        } else {
            OFF
        };
        self.values.value(state)
    }
}

/// A light that is not the control of its action: its values by state beside it (`"on":
/// 100`, `"playing": {"blink": [9, 0], "ms": 250}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileLight {
    pub target: TargetId,
    pub control: Control,
    /// While its group is in this mode only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
    /// Its values by the colour of its action, the nearest colour's (see the controls').
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub palette: BTreeMap<String, u8>,
    #[serde(flatten)]
    pub values: LightValues,
}

fn full() -> u8 {
    127
}

fn is_full(value: &u8) -> bool {
    *value == 127
}

fn is_zero(value: &u8) -> bool {
    *value == 0
}

fn unscaled() -> f64 {
    1.0
}

fn is_unscaled(scale: &f64) -> bool {
    *scale == 1.0
}

fn is_no_offset(offset: &f64) -> bool {
    *offset == 0.0
}

fn straight() -> f32 {
    1.0
}

/// A meter's curve, a power above 0: a level between silence and full scale stays between them
/// once raised to it.
fn curve<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    let curve = f32::deserialize(deserializer)?;
    if curve.is_finite() && curve > 0.0 {
        Ok(curve)
    } else {
        Err(de::Error::custom(format!("a meter's curve is above 0, not {curve}")))
    }
}

fn is_straight(curve: &f32) -> bool {
    *curve == 1.0
}

fn jog_steps_per_turn() -> f64 {
    JOG_STEPS_PER_TURN
}

fn is_jog_steps_per_turn(steps: &f64) -> bool {
    *steps == JOG_STEPS_PER_TURN
}

impl Profile {
    /// The profile of the model on the MIDI port `port` (or the HID device so named), of
    /// `profiles`: the one whose name the port holds that is the longest, the first of those as
    /// long.
    pub fn of_port<'p>(profiles: &'p [Profile], port: &str) -> Option<&'p Profile> {
        let port = port.to_lowercase();
        profiles
            .iter()
            .rev()
            .filter_map(|p| Some((p.fit(&port)?, p)))
            .max_by_key(|&(fit, _)| fit)
            .map(|(_, p)| p)
    }

    /// How closely the port `port` is this model: the length of the longest of its names the
    /// port holds, case aside; `None` if it holds none. A HID device is the model of its ids.
    pub fn fits(&self, port: &str) -> Option<usize> {
        self.fit(&port.to_lowercase())
    }

    /// [`Profile::fits`], `port` in lower case.
    fn fit(&self, port: &str) -> Option<usize> {
        match (hid::ids_of(port), &self.hid) {
            (Some(ids), Some(device)) => return (ids == (device.vendor_id, device.product_id)).then_some(1),
            (Some(_), None) | (None, Some(_)) => return None,
            (None, None) => {}
        }
        self.ports
            .iter()
            .map(|p| p.to_lowercase())
            .filter(|p| port.contains(p.as_str()))
            .map(|p| p.len())
            .max()
    }

    /// The mappings of this model on `device`: its controls, and its mode keys.
    pub fn mappings<'p>(&'p self, device: &'p str) -> impl Iterator<Item = Mapping> + 'p {
        let controls = self.controls.iter().map(move |c| Mapping::new(device, c.clone()));
        let selects = self.modes.iter().flat_map(move |g| {
            g.select.iter().map(move |s| {
                let binding = ProfileControl {
                    control: s.control,
                    target: Target::Mode { group: g.group.clone(), mode: s.mode.clone() }.into(),
                    shift: s.shift.clone(),
                    light: s.light.clone(),
                    held: s.held,
                    once: s.once,
                    ..Default::default()
                };
                Mapping::new(device, binding)
            })
        });
        controls.chain(selects)
    }

    /// The modes its groups begin in.
    pub fn first_modes(&self) -> Modes {
        self.modes.iter().filter_map(|g| Some((g.group.clone(), g.modes.first()?.clone()))).collect()
    }

    /// The controls it reads: those it maps, the keys of its chords and its mode keys.
    fn inputs(&self) -> impl Iterator<Item = Control> + '_ {
        let mapped = self
            .controls
            .iter()
            .flat_map(|c| std::iter::once(c.control).chain(c.chord.as_ref().map(|c| c.with)));
        mapped.chain(self.modes.iter().flat_map(|g| g.select.iter().map(|s| s.control)))
    }

    /// The fields of a HID controller's reports it reads.
    pub fn hid_inputs(&self) -> BTreeSet<Field> {
        self.inputs().filter_map(|c| if let Control::Hid(field) = c { Some(field) } else { None }).collect()
    }

    /// The lights and outputs it shows things on, but its keys' own.
    fn shown_on(&self) -> impl Iterator<Item = Control> + '_ {
        let meters = self.meters.iter().flat_map(|m| match &m.on {
            MeterOn::Control { control, .. } => vec![*control],
            MeterOn::Leds { leds } => leds.iter().map(|l| l.control).collect(),
        });
        let lights = self.lights.iter().map(|l| l.control);
        lights.chain(meters).chain(self.outputs.iter().map(|o| o.control))
    }

    /// What does not hold together in it, that its reading cannot tell: an action alone of a
    /// control that is no shift key, a MIDI control in a HID profile or the other way round, a
    /// HID light beyond its output reports, an input report asked that no control reads, a report
    /// sent at the wrong length.
    pub fn problems(&self) -> Vec<String> {
        let shift = |c: &ProfileControl| matches!(c.target.target(), Some(Target::Shift(_)));
        let mut problems: Vec<String> = self
            .controls
            .iter()
            .filter(|c| c.alone.is_some() && !shift(c))
            .map(|c| format!("{} has an action alone, though it is no shift", c.control))
            .collect();
        let Some(device) = &self.hid else {
            if self.inputs().chain(self.shown_on()).any(|c| matches!(c, Control::Hid(_))) {
                problems.push("a HID control in a profile of no HID device".to_owned());
            }
            return problems;
        };
        let lit = device.leds.iter().map(|l| l.led);
        for control in self.inputs().chain(device.leds.iter().map(|l| l.control)) {
            if !matches!(control, Control::Hid(_)) {
                problems.push(format!("{control} is not a field of a HID report"));
            }
        }
        for control in self.shown_on().chain(lit) {
            let report = match control {
                Control::Hid(led) => device.outputs.iter().find(|o| o.report == led.report).map(|o| (o, led)),
                _ => None,
            };
            if report.is_none_or(|(report, led)| led.span().end > usize::from(report.length)) {
                problems.push(format!("{control} is not a light of an output report"));
            }
        }
        let read: BTreeSet<u8> = self.hid_inputs().iter().map(|f| f.report).collect();
        for id in device.inputs.iter().filter(|id| !read.contains(id)) {
            problems.push(format!("input report {id} asked, though no control is a field of it"));
        }
        let messages =
            self.init.iter().map(|m| m.when_and_what().1).chain(self.close.iter().map(Vec::as_slice));
        for message in messages.chain(self.keep_alive.iter().map(|k| k.message.as_slice())) {
            let declared = message.first().and_then(|id| device.outputs.iter().find(|o| o.report == *id));
            if let Some(o) = declared.filter(|o| usize::from(o.length) != message.len()) {
                problems.push(format!(
                    "report {} sent with {} bytes, not its {}",
                    o.report,
                    message.len(),
                    o.length
                ));
            }
        }
        problems
    }
}

impl ProfileLight {
    /// Its value in `state`, while its group is in `modes`.
    pub fn value(&self, state: &str, modes: &Modes) -> Option<&LightValue> {
        let applies = self.when.as_ref().is_none_or(|w| modes.get(&w.group) == Some(&w.mode));
        applies.then(|| self.values.value(state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapping::Encoding;

    const PLAY: Control = Control::note(0, 11);

    fn profile() -> Profile {
        serde_json::from_str(
            r#"{
                "name": "Pioneer DDJ-SB2",
                "ports": ["DDJ-SB2"],
                "init": [[155, 9, 127]],
                "controls": [
                    { "control": { "kind": "note", "channel": 0, "number": 11 }, "target": "deck1.play" },
                    { "control": { "kind": "controlChange", "channel": 0, "number": 0 }, "target": "deck1.pitch", "fine": 32 },
                    { "control": { "kind": "controlChange", "channel": 0, "number": 33 }, "target": "deck1.jog", "encoding": "relativeOffset" }
                ],
                "lights": [{ "target": "deck1.play", "control": { "kind": "note", "channel": 0, "number": 71 }, "on": 100 }]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn a_profile_fits_its_ports_on_every_system() {
        let profiles = [profile()];
        let name = |port| Profile::of_port(&profiles, port).map(|p| p.name.as_str());
        assert_eq!(name("DDJ-SB2"), Some("Pioneer DDJ-SB2"));
        assert_eq!(name("PIONEER DDJ-SB2:PIONEER DDJ-SB2 MIDI 1 24:0"), Some("Pioneer DDJ-SB2"));
        assert_eq!(name("2- ddj-sb2"), Some("Pioneer DDJ-SB2"));
        assert_eq!(name("DDJ-SB3"), None);
    }

    #[test]
    fn the_longest_name_a_port_holds_tells_its_model() {
        let older = Profile { name: "Pioneer DDJ-SB".into(), ports: vec!["DDJ-SB".into()], ..profile() };
        let profiles = [older, profile()];
        let name = |port| Profile::of_port(&profiles, port).map(|p| p.name.as_str());
        assert_eq!(name("PIONEER DDJ-SB2"), Some("Pioneer DDJ-SB2"));
        assert_eq!(name("PIONEER DDJ-SB"), Some("Pioneer DDJ-SB"));
    }

    #[test]
    fn a_profile_gives_its_mappings_to_a_device() {
        let p = profile();
        let mappings: Vec<Mapping> = p.mappings("DDJ-SB2").collect();
        assert_eq!(mappings.len(), 3);
        assert_eq!(mappings[0].control, PLAY);
        assert_eq!(mappings[0].device, "DDJ-SB2");
        assert_eq!(mappings[1].fine, Some(32));
        assert_eq!(mappings[2].encoding, Encoding::RelativeOffset);
        assert_eq!(p.init, [InitMessage::Now(vec![155, 9, 127])]);
    }

    #[test]
    fn a_hid_controller_is_the_model_of_its_ids_and_no_port_names_it() {
        let s4: Profile = serde_json::from_str(
            r#"{
                "name": "Native Instruments Traktor Kontrol S4 MK2",
                "hid": { "vendorId": 6092, "productId": 4880 },
                "controls": [{ "control": { "kind": "hid", "report": 1, "byte": 13 }, "target": "deck1.play" }]
            }"#,
        )
        .unwrap();
        let s2 = Profile { name: "S2".into(), ports: vec!["S4".into()], hid: None, ..s4.clone() };
        let profiles = [s2, s4];
        let plugged = hid::device_name("Traktor Kontrol S4 MK2", 0x17cc, 0x1310);
        let name = |port| Profile::of_port(&profiles, port).map(|p| p.name.as_str());
        assert_eq!(name(&plugged), Some(profiles[1].name.as_str()));
        assert_eq!(name("Kontrol S4 MIDI"), Some("S2"), "a HID profile names no port");
        assert_eq!(name("Traktor Kontrol S4 (HID 17cc:1111)"), None, "a MIDI profile no HID device");
        assert!(!serde_json::to_string(&profiles[1]).unwrap().contains("ports"));
    }

    #[test]
    fn a_hid_profile_holds_together() {
        let profile = |lights: &str, init: &str| -> Profile {
            serde_json::from_str(&format!(
                r#"{{
                    "name": "S4",
                    "hid": {{ "vendorId": 1, "productId": 2, "outputs": [{{ "report": 128, "length": 4 }}],
                        "leds": [{{ "control": {{ "kind": "hid", "report": 1, "byte": 1 }}, "led": {{ "kind": "hid", "report": 128, "byte": 1 }} }}] }},
                    "init": {init},
                    "controls": [{{ "control": {{ "kind": "hid", "report": 1, "byte": 1 }}, "target": "deck1.play" }}],
                    "lights": [{lights}]
                }}"#
            ))
            .unwrap()
        };
        let light = |byte: u16, bits: u8| {
            format!(
                r#"{{ "target": "deck1.sync", "control": {{ "kind": "hid", "report": 128, "byte": {byte}, "bits": {bits} }} }}"#
            )
        };
        assert_eq!(profile(&light(1, 24), "[[128, 0, 0, 0]]").problems(), Vec::<String>::new());
        assert_eq!(
            profile(&light(2, 24), "[]").problems(),
            ["report 128, byte 2, 24 bits is not a light of an output report"]
        );
        assert_eq!(
            profile(&light(1, 8), "[[128, 0]]").problems(),
            ["report 128 sent with 2 bytes, not its 4"]
        );
        let midi = r#"{ "target": "deck1.sync", "control": { "kind": "note", "channel": 0, "number": 1 } }"#;
        assert_eq!(profile(midi, "[]").problems(), ["note 1, channel 1 is not a light of an output report"]);
        let mut p = profile(&light(1, 8), "[]");
        p.hid.as_mut().unwrap().inputs = vec![1, 2];
        assert_eq!(p.problems(), ["input report 2 asked, though no control is a field of it"]);
        p.hid = None;
        assert_eq!(p.problems(), ["a HID control in a profile of no HID device"]);
        assert_eq!(p.hid_inputs().len(), 1);
    }

    #[test]
    fn only_a_shift_key_has_an_action_alone_and_a_control_may_be_named() {
        let mut p: Profile = serde_json::from_str(
            r#"{
                "name": "Behringer CMD DV-1",
                "ports": ["CMD DV-1"],
                "controls": [
                    { "control": { "kind": "note", "channel": 6, "number": 85 }, "target": "shift.erase", "name": "ERASE", "alone": "general.close" },
                    { "control": { "kind": "note", "channel": 6, "number": 86 }, "target": "library.load1", "name": "STORE" }
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(p.problems(), Vec::<String>::new());
        assert_eq!(p.controls[1].name.as_deref(), Some("STORE"));
        p.controls[1].alone = Some("general.close".into());
        assert_eq!(p.problems(), ["note 86, channel 7 has an action alone, though it is no shift"]);
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<Profile>(&json).unwrap(), p);
    }

    #[test]
    fn a_jog_wheel_turns_by_the_steps_of_its_model() {
        assert_eq!(profile().jog_steps_per_turn, JOG_STEPS_PER_TURN);
        let measured = Profile { jog_steps_per_turn: 2048.0, ..profile() };
        let json = serde_json::to_string(&measured).unwrap();
        assert!(json.contains(r#""jogStepsPerTurn":2048"#), "{json}");
        assert_eq!(serde_json::from_str::<Profile>(&json).unwrap(), measured);
    }

    #[test]
    fn a_light_takes_its_own_values() {
        let light = &profile().lights[0];
        assert_eq!(light.value("on", &Modes::new()), Some(&LightValue::Steady(100)));
        assert_eq!(light.value("off", &Modes::new()), Some(&LightValue::Steady(0)));
    }

    #[test]
    fn mode_keys_become_mappings_and_the_first_mode_is_where_a_group_begins() {
        let p: Profile = serde_json::from_str(
            r#"{
                "name": "Numark Mixtrack Pro FX",
                "ports": ["Mixtrack Pro FX"],
                "modes": [{
                    "group": "padsA",
                    "modes": ["hotcue", "roll"],
                    "select": [
                        { "control": { "kind": "note", "channel": 4, "number": 0 }, "mode": "hotcue" },
                        { "control": { "kind": "note", "channel": 4, "number": 13 }, "mode": "roll", "light": { "off": 1 } }
                    ]
                }],
                "controls": [
                    { "control": { "kind": "note", "channel": 4, "number": 20 }, "target": "deck1.hotcue1", "when": { "group": "padsA", "mode": "hotcue" } },
                    { "control": { "kind": "note", "channel": 4, "number": 20 }, "target": "deck1.roll1", "when": { "group": "padsA", "mode": "roll" } }
                ],
                "meters": [{ "source": "deck1", "control": { "kind": "controlChange", "channel": 0, "number": 31 }, "max": 90 }],
                "keepAlive": { "everyMs": 200, "message": [240, 0, 64, 5, 247] }
            }"#,
        )
        .unwrap();
        let mappings: Vec<Mapping> = p.mappings("Mixtrack").collect();
        let targets: Vec<&str> = mappings.iter().map(|m| m.target.as_str()).collect();
        assert_eq!(targets, ["deck1.hotcue1", "deck1.roll1", "mode.padsA.hotcue", "mode.padsA.roll"]);
        assert_eq!(*mappings[3].light.value("off"), LightValue::Steady(1));
        assert_eq!(p.first_modes(), Modes::from([("padsA".into(), "hotcue".into())]));
        assert_eq!(value(&p.meters[0], 0.5), 45);
        assert_eq!(p.keep_alive.as_ref().map(|k| k.every_ms), Some(200));
    }

    fn meter(json: &str) -> Meter {
        serde_json::from_str(json).unwrap()
    }

    /// What `meter` shows at `level`, `clipping` or not.
    fn shown(meter: &Meter, level: f32, clipping: bool) -> Vec<(Control, LightValue)> {
        let mut shown = Vec::new();
        meter.show(level, clipping, |control, value| shown.push((control, value.clone())));
        shown
    }

    /// The value a meter on one control shows at `level`.
    fn value(meter: &Meter, level: f32) -> u8 {
        match shown(meter, level, false)[..] {
            [(_, LightValue::Steady(value))] => value,
            ref shown => panic!("not a value on one control: {shown:?}"),
        }
    }

    #[test]
    fn a_meter_follows_its_curve() {
        let meter = meter(
            r#"{ "source": "main", "control": { "kind": "note", "channel": 0, "number": 11 }, "curve": 2.5 }"#,
        );
        assert_eq!(value(&meter, 1.0), 127);
        assert_eq!(value(&meter, 0.5), 22);
        assert_eq!(value(&meter, 2.0), 127, "past full scale, full");
    }

    #[test]
    fn a_meter_curve_is_above_0() {
        for curve in ["0", "-1"] {
            let text = format!(
                r#"{{ "source": "main", "control": {{ "kind": "note", "channel": 0, "number": 11 }}, "steps": [0, 127], "curve": {curve} }}"#
            );
            assert!(serde_json::from_str::<Meter>(&text).is_err(), "{curve}");
        }
    }

    #[test]
    fn a_meter_starts_at_its_least_value_or_shows_steps() {
        let control = r#""control": { "kind": "controlChange", "channel": 0, "number": 126 }"#;
        let offset = meter(&format!(r#"{{ "source": "deck1", {control}, "min": 48, "max": 63 }}"#));
        assert_eq!((value(&offset, 0.0), value(&offset, 1.0)), (48, 63));
        let stepped =
            meter(&format!(r#"{{ "source": "deck1", {control}, "steps": [0, 1, 21, 41, 61, 81] }}"#));
        assert_eq!([0.0, 0.2, 0.5, 1.0].map(|l| value(&stepped, l)), [0, 1, 41, 81]);
    }

    #[test]
    fn a_meter_shows_its_clip_value_while_clipping() {
        let meter = meter(
            r#"{ "source": "deck1", "control": { "kind": "controlChange", "channel": 0, "number": 2 }, "max": 117, "clip": 119 }"#,
        );
        assert_eq!(shown(&meter, 0.5, true), [(meter_control(0, 2), LightValue::Steady(119))]);
    }

    fn meter_control(channel: u8, number: u8) -> Control {
        Control::control_change(channel, number)
    }

    fn leds() -> Meter {
        meter(
            r#"{ "source": "deck1", "leds": [
                { "control": { "kind": "controlChange", "channel": 0, "number": 83 }, "from": 0.2 },
                { "control": { "kind": "controlChange", "channel": 0, "number": 84 }, "from": 0.6, "on": 6, "off": 1 },
                { "control": { "kind": "controlChange", "channel": 0, "number": 85 }, "from": 0.9, "on": [176, 80, 10], "off": [176, 81, 10], "clip": 127 }
            ] }"#,
        )
    }

    fn lit(meter: &Meter, level: f32, clipping: bool) -> Vec<LightValue> {
        shown(meter, level, clipping).into_iter().map(|(_, value)| value).collect()
    }

    #[test]
    fn each_led_of_a_meter_lights_from_its_own_share_with_a_lights_values() {
        let meter = leds();
        let controls: Vec<Control> = shown(&meter, 0.0, false).into_iter().map(|(c, _)| c).collect();
        assert_eq!(controls, [83, 84, 85].map(|n| meter_control(0, n)));
        let [unlit, lit_message] = [81, 80].map(|on| LightValue::Message(vec![176, on, 10]));
        let steady = LightValue::Steady;
        assert_eq!(lit(&meter, 0.0, false), [steady(0), steady(1), unlit.clone()]);
        assert_eq!(lit(&meter, 0.2, false), [steady(127), steady(1), unlit]);
        assert_eq!(lit(&meter, 0.9, false), [steady(127), steady(6), lit_message.clone()]);
        assert_eq!(lit(&meter, 3.0, false), [steady(127), steady(6), lit_message]);
    }

    #[test]
    fn a_meter_of_leds_follows_its_curve_and_shows_their_clip_values() {
        let curved = Meter { curve: 2.0, ..leds() };
        assert_eq!(lit(&curved, 0.5, false)[..2], [LightValue::Steady(127), LightValue::Steady(1)]);
        let steady = LightValue::Steady;
        assert_eq!(
            lit(&curved, 0.5, true),
            [steady(127), steady(1), steady(127)],
            "the others show the level"
        );
    }

    #[test]
    fn a_meter_is_on_one_control_or_on_leds_and_survives_a_round_trip() {
        let on_leds = leds();
        assert!(matches!(&on_leds.on, MeterOn::Leds { leds } if leds.len() == 3));
        let json = serde_json::to_string(&on_leds).unwrap();
        assert_eq!(serde_json::from_str::<Meter>(&json).unwrap(), on_leds);
        let on_control = meter(
            r#"{ "source": "main", "control": { "kind": "note", "channel": 0, "number": 11 }, "curve": 0.5, "when": { "group": "left", "mode": "deck1" } }"#,
        );
        assert!(matches!(on_control.on, MeterOn::Control { max: 127, .. }));
        let json = serde_json::to_string(&on_control).unwrap();
        assert!(!json.contains("leds") && !json.contains("min"), "{json}");
        assert_eq!(serde_json::from_str::<Meter>(&json).unwrap(), on_control);
        assert!(serde_json::from_str::<Meter>(r#"{ "source": "main" }"#).is_err(), "shown nowhere");
    }

    #[test]
    fn init_messages_go_at_once_or_later() {
        let init: Vec<InitMessage> =
            serde_json::from_str(r#"[[176, 1, 2], { "afterMs": 1500, "message": [240, 1, 247] }]"#).unwrap();
        assert_eq!(init[0].when_and_what(), (0, &[176, 1, 2][..]));
        assert_eq!(init[1].when_and_what(), (1500, &[240, 1, 247][..]));
    }

    #[test]
    fn an_output_spans_its_values() {
        let output: Output = serde_json::from_str(
            r#"{ "source": "deck1.position", "control": { "kind": "controlChange", "channel": 0, "number": 6 }, "min": 1, "max": 21 }"#,
        )
        .unwrap();
        assert_eq!([0.0, 0.5, 1.0].map(|p| output.value(p)), [1, 11, 21]);
    }

    #[test]
    fn an_output_shows_nothing_dark_below_its_values_else_at_the_start_of_its_travel() {
        let off = |min, max| {
            Output { min, max, ..serde_json::from_str::<Output>(r#"{ "source": "deck1.turn", "control": { "kind": "controlChange", "channel": 0, "number": 6 } }"#).unwrap() }.off()
        };
        assert_eq!(off(1, 15), 0, "a ring of 15 LEDs");
        assert_eq!(off(29, 21), 0, "upside down");
        assert_eq!(off(0, 127), 0);
        assert_eq!(off(127, 0), 127, "upside down from 0");
    }

    #[test]
    fn a_display_writes_its_number_in_nibbles_or_in_a_byte() {
        let display = |json: &str| serde_json::from_str::<Display>(json).unwrap();
        let bpm = display(
            r#"{ "source": "deck1.bpm", "message": [240, 0, 32, 127, 1, 1, "value", 247], "scale": 100, "nibbles": 6 }"#,
        );
        assert_eq!(bpm.message(124.37), [240, 0, 32, 127, 1, 1, 0, 0, 3, 0, 9, 5, 247]);
        let time = display(
            r#"{ "source": "deck1.remaining", "message": ["value"], "scale": -1000, "nibbles": 8, "sign": [8, 7] }"#,
        );
        assert_eq!(time.message(1.5), [7, 15, 15, 15, 15, 10, 2, 4]);
        assert_eq!(time.message(-1.5)[..2], [8, 0]);
        let pitch = display(
            r#"{ "source": "deck1.tempo", "message": ["value"], "scale": 100, "offset": 8388608, "nibbles": 6 }"#,
        );
        assert_eq!(pitch.message(-1.0), [7, 15, 15, 15, 9, 12]);
        let range = display(r#"{ "source": "deck1.tempoRange", "message": [144, 14, "value"] }"#);
        assert_eq!([range.message(8.0), range.message(300.0)], [[144, 14, 8], [144, 14, 127]]);
        assert_eq!(serde_json::from_str::<Display>(&serde_json::to_string(&bpm).unwrap()).unwrap(), bpm);
    }

    #[test]
    fn a_profile_survives_a_round_trip() {
        let p = profile();
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("absolute"), "defaults are left out: {json}");
        assert_eq!(serde_json::from_str::<Profile>(&json).unwrap(), p);
    }
}
