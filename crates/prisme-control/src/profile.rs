use crate::light::{LightValue, LightValues};
use crate::mapping::{Encoding, Mapping, Modes, When, MODE};
use crate::message::Control;
use serde::{Deserialize, Serialize};

/// Steps in a turn of a jog wheel whose model does not tell. Most controllers send
/// hundreds; the number only scales scratches and nudges.
pub const JOG_STEPS_PER_TURN: f64 = 720.0;

/// What Prisme knows of a controller model before anything is learnt on it: the controls
/// it maps, the lights beyond those controls' own, and what it is sent once opened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
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
    /// Sent when the controller is opened (a request for its knobs' positions, say).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub init: Vec<Vec<u8>>,
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
}

/// A message sent every `every_ms` milliseconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeepAlive {
    pub every_ms: u32,
    pub message: Vec<u8>,
}

/// A group of controls and its modes, the first one to begin with, each chosen by a key
/// whose light shows whether it is the one (`on`, `off`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeGroup {
    pub group: String,
    pub modes: Vec<String>,
    pub select: Vec<ModeSelect>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeSelect {
    pub control: Control,
    pub mode: String,
    /// Chosen while the controller's shift is held.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shift: bool,
    #[serde(default, skip_serializing_if = "LightValues::is_empty")]
    pub light: LightValues,
}

/// A level shown on the controller: `source`'s (`deck1`…, `main`), from 0 to `max` on
/// `control`, along `curve` (the level raised to it: 1 straight).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meter {
    pub source: String,
    pub control: Control,
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub max: u8,
    #[serde(default = "straight", skip_serializing_if = "is_straight")]
    pub curve: f32,
}

impl Meter {
    /// The value showing `level` (full scale 1).
    pub fn value(&self, level: f32) -> u8 {
        (level.clamp(0.0, 1.0).powf(self.curve) * f32::from(self.max)).round() as u8
    }
}

/// A control of the model and the action it drives: a [`Mapping`] before a device has it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileControl {
    pub control: Control,
    pub target: String,
    #[serde(default, skip_serializing_if = "is_absolute")]
    pub encoding: Encoding,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fine: Option<u8>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shift: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub invert: bool,
    /// Its light's values, when they are not 127 and 0.
    #[serde(default, skip_serializing_if = "LightValues::is_empty")]
    pub light: LightValues,
}

/// A light that is not the control of its action: its values by state beside it (`"on":
/// 100`, `"playing": {"blink": [9, 0], "ms": 250}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileLight {
    pub target: String,
    pub control: Control,
    /// While its group is in this mode only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
    #[serde(flatten)]
    pub values: LightValues,
}

fn is_absolute(encoding: &Encoding) -> bool {
    *encoding == Encoding::Absolute
}

fn full() -> u8 {
    127
}

fn is_full(value: &u8) -> bool {
    *value == 127
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

    /// How closely the port `port` (in lower case) is this model: the length of the longest
    /// of its names the port holds; `None` if it holds none.
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
        let controls = self.controls.iter().map(move |c| Mapping {
            device: device.to_owned(),
            control: c.control,
            target: c.target.clone(),
            encoding: c.encoding,
            fine: c.fine,
            shift: c.shift,
            when: c.when.clone(),
            invert: c.invert,
            light: c.light.clone(),
        });
        let selects = self.modes.iter().flat_map(move |g| {
            g.select.iter().map(move |s| Mapping {
                device: device.to_owned(),
                control: s.control,
                target: format!("{MODE}{}.{}", g.group, s.mode),
                encoding: Encoding::Absolute,
                fine: None,
                shift: s.shift,
                when: None,
                invert: false,
                light: s.light.clone(),
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
        assert_eq!(p.init, [vec![155, 9, 127]]);
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

    #[test]
    fn a_meter_follows_its_curve() {
        let meter = Meter { source: "main".into(), control: PLAY, max: 127, curve: 2.5 };
        assert_eq!(meter.value(1.0), 127);
        assert_eq!(meter.value(0.5), 22);
        assert_eq!(meter.value(2.0), 127, "past full scale, full");
    }

    #[test]
    fn a_profile_survives_a_round_trip() {
        let p = profile();
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("absolute"), "defaults are left out: {json}");
        assert_eq!(serde_json::from_str::<Profile>(&json).unwrap(), p);
    }
}
