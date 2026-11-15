use crate::mapping::{Encoding, Mapping};
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
    pub controls: Vec<ProfileControl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lights: Vec<ProfileLight>,
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
}

/// A light that is not the control of its action, or that takes other values than 127 and 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileLight {
    pub target: String,
    pub control: Control,
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub on: u8,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub off: u8,
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

fn is_zero(value: &u8) -> bool {
    *value == 0
}

fn jog_steps_per_turn() -> f64 {
    JOG_STEPS_PER_TURN
}

fn is_jog_steps_per_turn(steps: &f64) -> bool {
    *steps == JOG_STEPS_PER_TURN
}

impl Profile {
    /// The profile of the model on the MIDI port `port`, of `profiles`: the one whose name
    /// the port holds that is the longest.
    pub fn of_port<'p>(profiles: &'p [Profile], port: &str) -> Option<&'p Profile> {
        let port = port.to_lowercase();
        profiles.iter().filter_map(|p| Some((p.fit(&port)?, p))).max_by_key(|&(fit, _)| fit).map(|(_, p)| p)
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

    /// The mappings of this model on `device`.
    pub fn mappings<'p>(&'p self, device: &'p str) -> impl Iterator<Item = Mapping> + 'p {
        self.controls.iter().map(move |c| Mapping {
            device: device.to_owned(),
            control: c.control,
            target: c.target.clone(),
            encoding: c.encoding,
            fine: c.fine,
            shift: c.shift,
        })
    }
}

impl ProfileLight {
    /// The message that switches this light on or off.
    pub fn message(&self, on: bool) -> Option<[u8; 3]> {
        let value = if on { self.on } else { self.off };
        crate::mapping::light(self.control, true).map(|[status, number, _]| [status, number, value])
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
        assert_eq!(light.message(true), Some([0x90, 71, 100]));
        assert_eq!(light.message(false), Some([0x90, 71, 0]));
    }

    #[test]
    fn a_profile_survives_a_round_trip() {
        let p = profile();
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("absolute"), "defaults are left out: {json}");
        assert_eq!(serde_json::from_str::<Profile>(&json).unwrap(), p);
    }
}
