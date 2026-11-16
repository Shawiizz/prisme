//! The JSON Schema of the profiles, for the editors that check and complete them: the format
//! as Prisme reads it, its actions named.

use crate::profile::Profile;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Where the schema is published: a profile names it as its `$schema`.
pub const SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/Shawiizz/prisme/main/apps/prisme-desktop/controllers/profile.schema.json";

/// An action of Prisme a control can drive, as the application lists them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub id: String,
    pub group: String,
    pub label: String,
    /// `button`, `hold`, `fader`, `encoder`, `direct`.
    pub kind: String,
}

/// The actions named by what they measure, the modes of a profile's groups and its named shifts.
const PATTERNS: [(&str, &str); 5] = [
    (
        r"^deck[1-4]\.(jump|loop|roll)\.[+-]?\d+(\.\d+)?$",
        "Jumps, loops or rolls of so many beats (`deck1.jump.-8`, `deck1.loop.0.25`)",
    ),
    (r"^deck[1-4]\.key\.[+-]?\d+$", "The key moved by so many semitones (`deck1.key.+2`)"),
    (r"^deck[1-4]\.sample([1-9]|1[0-6])\.velocity$", "A sample played as loud as the pad is hit"),
    (r"^mode\.[^.]+\.[^.]+$", "A mode of one of the profile's groups (`mode.<group>.<mode>`)"),
    (r"^shift\.[^.]+$", "A named shift (`shift.left`), one per side say: a shift too"),
];

/// The schema of a profile, `targets` the actions its controls can drive.
pub fn profile_schema(targets: &[Target]) -> Value {
    let mut schema = serde_json::to_value(schemars::schema_for!(Profile)).expect("a schema is JSON");
    schema["$id"] = SCHEMA_URL.into();
    schema["title"] = "Prisme controller profile".into();
    let defs = &mut schema["$defs"];
    defs["Target"] = target(targets);
    let target = |described: &Value| {
        let mut target = json!({ "$ref": "#/$defs/Target" });
        if let Some(what) = described.get("description") {
            target["description"] = what.clone();
        }
        target
    };
    for def in ["ProfileControl", "ProfileLight", "Hold"] {
        let property = &mut defs[def]["properties"]["target"];
        *property = target(property);
    }
    let also = &mut defs["ProfileControl"]["properties"]["also"]["items"];
    *also = target(also);
    defs["Meter"]["properties"]["source"]["enum"] =
        json!(["deck1", "deck2", "deck3", "deck4", "main", "main.left", "main.right"]);
    schema
}

/// An action: one Prisme lists, or one named by what it measures.
fn target(targets: &[Target]) -> Value {
    let listed = json!({
        "enum": targets.iter().map(|t| &t.id).collect::<Vec<_>>(),
        // the editors that show what each one does (VS Code)
        "enumDescriptions": targets.iter().map(|t| format!("{}: {} ({})", t.group, t.label, t.kind)).collect::<Vec<_>>(),
    });
    let measured = PATTERNS.iter().map(|(pattern, what)| json!({ "pattern": pattern, "description": what }));
    json!({
        "description": "An action of Prisme",
        "type": "string",
        "anyOf": std::iter::once(listed).chain(measured).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets() -> Vec<Target> {
        let target = |id: &str, kind: &str| Target {
            id: id.into(),
            group: "Deck A".into(),
            label: "Play".into(),
            kind: kind.into(),
        };
        vec![target("deck1.play", "button"), target("deck1.jog", "direct")]
    }

    #[test]
    fn the_schema_names_the_actions_and_the_patterns() {
        let schema = profile_schema(&targets());
        assert_eq!(schema["$id"], SCHEMA_URL);
        let target = &schema["$defs"]["Target"];
        assert_eq!(target["anyOf"][0]["enum"], json!(["deck1.play", "deck1.jog"]));
        assert_eq!(target["anyOf"].as_array().map(Vec::len), Some(1 + PATTERNS.len()));
        let control = &schema["$defs"]["ProfileControl"]["properties"];
        assert_eq!(control["target"]["$ref"], "#/$defs/Target");
        assert_eq!(control["also"]["items"]["$ref"], "#/$defs/Target");
        assert!(schema["properties"]["controls"].is_object());
    }

    #[test]
    fn the_measured_patterns_read_as_regular_expressions() {
        for (pattern, _) in PATTERNS {
            assert!(pattern.starts_with('^') && pattern.ends_with('$'), "{pattern}");
        }
    }
}
