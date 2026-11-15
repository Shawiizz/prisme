use crate::light::{LightValue, LightValues};
use crate::mapping::{Mapping, Modes, ProfileControl, Shift, When, MODE};
use crate::message::Control;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The version of the profile format, raised when profiles may use what an older Prisme does
/// not read: a profile published after it was raised is left to the Prisme that reads it.
pub const PROFILE_FORMAT: u32 = 1;

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
    /// several models, the one whose name is the longest (a DDJ-SB2 is no DDJ-SB).
    pub ports: Vec<String>,
    /// Steps in a turn of its jog wheels, measured on the controller.
    #[serde(default = "jog_steps_per_turn", skip_serializing_if = "is_jog_steps_per_turn")]
    pub jog_steps_per_turn: f64,
    /// Where the mapping comes from, when it is not Prisme's own.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub credits: String,
    /// Tried on the controller itself, not only read from another program's mapping.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub verified: bool,
    /// Sent when the controller is opened (a request for its knobs' positions, say), some a
    /// while after.
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
/// `max`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    pub source: String,
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
/// `main.right`), from `min` to `max` on `control` along `curve` (the level raised to it: 1
/// straight), or as one of `steps` (the values at silence, then at each equal share of the
/// level); `clip` for a while after the level reached full scale.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Meter {
    pub source: String,
    pub control: Control,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub min: u8,
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub max: u8,
    #[serde(default = "straight", skip_serializing_if = "is_straight")]
    pub curve: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<u8>,
    /// While its group is in this mode only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
}

impl Meter {
    /// The value showing `level` (full scale 1).
    pub fn value(&self, level: f32) -> u8 {
        let shaped = level.clamp(0.0, 1.0).powf(self.curve);
        if let Some(last) = self.steps.len().checked_sub(1) {
            return self.steps[(shaped * last as f32).round() as usize];
        }
        let span = f32::from(self.max) - f32::from(self.min);
        (f32::from(self.min) + shaped * span).round() as u8
    }
}

/// A light that is not the control of its action: its values by state beside it (`"on":
/// 100`, `"playing": {"blink": [9, 0], "ms": 250}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileLight {
    pub target: String,
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

fn straight() -> f32 {
    1.0
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
    /// The profile of the model on the MIDI port `port`, of `profiles`: the one whose name
    /// the port holds that is the longest, the first of those as long.
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
    /// port holds, case aside; `None` if it holds none.
    pub fn fits(&self, port: &str) -> Option<usize> {
        self.fit(&port.to_lowercase())
    }

    /// [`Profile::fits`], `port` in lower case.
    fn fit(&self, port: &str) -> Option<usize> {
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
                    target: format!("{MODE}{}.{}", g.group, s.mode),
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
}

impl ProfileLight {
    /// Its value in `state`, while its group is in `modes`.
    pub fn value(&self, state: &str, modes: &Modes) -> Option<LightValue> {
        let applies = self.when.as_ref().is_none_or(|w| modes.get(&w.group) == Some(&w.mode));
        applies.then(|| self.values.value(state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapping::Encoding;
    use crate::message::ControlKind;

    const PLAY: Control = Control { kind: ControlKind::Note, channel: 0, number: 11 };

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
        assert_eq!(light.value("on", &Modes::new()), Some(LightValue::Steady(100)));
        assert_eq!(light.value("off", &Modes::new()), Some(LightValue::Steady(0)));
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
        assert_eq!(mappings[3].light.value("off"), LightValue::Steady(1));
        assert_eq!(p.first_modes(), Modes::from([("padsA".into(), "hotcue".into())]));
        assert_eq!(p.meters[0].value(0.5), 45);
        assert_eq!(p.keep_alive.as_ref().map(|k| k.every_ms), Some(200));
    }

    fn meter(json: &str) -> Meter {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_meter_follows_its_curve() {
        let meter = meter(
            r#"{ "source": "main", "control": { "kind": "note", "channel": 0, "number": 11 }, "curve": 2.5 }"#,
        );
        assert_eq!(meter.value(1.0), 127);
        assert_eq!(meter.value(0.5), 22);
        assert_eq!(meter.value(2.0), 127, "past full scale, full");
    }

    #[test]
    fn a_meter_starts_at_its_least_value_or_shows_steps() {
        let control = r#""control": { "kind": "controlChange", "channel": 0, "number": 126 }"#;
        let offset = meter(&format!(r#"{{ "source": "deck1", {control}, "min": 48, "max": 63 }}"#));
        assert_eq!((offset.value(0.0), offset.value(1.0)), (48, 63));
        let stepped =
            meter(&format!(r#"{{ "source": "deck1", {control}, "steps": [0, 1, 21, 41, 61, 81] }}"#));
        assert_eq!([0.0, 0.2, 0.5, 1.0].map(|l| stepped.value(l)), [0, 1, 41, 81]);
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
    fn a_profile_survives_a_round_trip() {
        let p = profile();
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("absolute"), "defaults are left out: {json}");
        assert_eq!(serde_json::from_str::<Profile>(&json).unwrap(), p);
    }
}
