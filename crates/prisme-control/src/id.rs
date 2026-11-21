//! What a profile or a learnt mapping names by an id: the id as written, and what it names,
//! read once.

use crate::source::Source;
use crate::target::Target;
use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

/// An id as written, and what it names if this version of Prisme knows it: an id it does not
/// know drives or shows nothing, and is kept as written.
#[derive(Debug, Clone)]
pub struct Id<T> {
    id: String,
    named: Option<T>,
}

/// An action a control drives (`deck1.play`, `mode.pads.roll`).
pub type TargetId = Id<Target>;

/// What a value shown on a controller is (`deck1.position`, `deck1.volume`).
pub type SourceId = Id<Source>;

impl<T> Id<T> {
    pub fn as_str(&self) -> &str {
        &self.id
    }

    /// What it names, if Prisme knows it.
    pub fn named(&self) -> Option<&T> {
        self.named.as_ref()
    }
}

impl TargetId {
    /// The action, if Prisme knows it.
    pub fn target(&self) -> Option<&Target> {
        self.named()
    }

    /// The group and the mode it selects, if it is a mode's.
    pub fn mode(&self) -> Option<(&str, &str)> {
        match &self.named {
            Some(Target::Mode { group, mode }) => Some((group.as_str(), mode.as_str())),
            _ => None,
        }
    }
}

impl<T> Default for Id<T> {
    fn default() -> Self {
        Self { id: String::new(), named: None }
    }
}

impl<T: FromStr> From<String> for Id<T> {
    fn from(id: String) -> Self {
        let named = id.parse().ok();
        Self { id, named }
    }
}

impl<T: FromStr> From<&str> for Id<T> {
    fn from(id: &str) -> Self {
        id.to_owned().into()
    }
}

impl From<Target> for TargetId {
    fn from(target: Target) -> Self {
        Self { id: target.to_string(), named: Some(target) }
    }
}

impl<T> From<Id<T>> for String {
    fn from(id: Id<T>) -> Self {
        id.id
    }
}

impl<T> std::ops::Deref for Id<T> {
    type Target = str;

    fn deref(&self) -> &str {
        &self.id
    }
}

impl<T> fmt::Display for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.id)
    }
}

impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Eq for Id<T> {}

impl<T> Hash for Id<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T> PartialEq<str> for Id<T> {
    fn eq(&self, other: &str) -> bool {
        self.id == other
    }
}

impl<T> PartialEq<&str> for Id<T> {
    fn eq(&self, other: &&str) -> bool {
        self.id == *other
    }
}

impl<T> PartialEq<String> for Id<T> {
    fn eq(&self, other: &String) -> bool {
        self.id == *other
    }
}

impl<T> Serialize for Id<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.id)
    }
}

impl<'de, T: FromStr> Deserialize<'de> for Id<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::from)
    }
}

/// Written as its id.
#[cfg(feature = "ts")]
impl<T> ts_rs::TS for Id<T> {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;

    fn name(_: &ts_rs::Config) -> String {
        "string".to_owned()
    }

    fn inline(cfg: &ts_rs::Config) -> String {
        Self::name(cfg)
    }
}

/// Written as its id: the schema of a profile names the actions apart (`schema`).
impl<T> JsonSchema for Id<T> {
    fn schema_name() -> Cow<'static, str> {
        "Id".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "string" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Measure;
    use crate::target::DeckAction;

    #[test]
    fn an_id_is_kept_as_written_whether_known_or_not() {
        let known = TargetId::from("deck1.key.+2");
        assert_eq!(known.target(), Some(&Target::Deck(0, DeckAction::KeyTo(2))));
        assert_eq!(serde_json::to_string(&known).unwrap(), r#""deck1.key.+2""#);
        let unknown: TargetId = serde_json::from_str(r#""deck1.teleport""#).unwrap();
        assert_eq!(unknown.target(), None);
        assert_eq!(unknown, "deck1.teleport");
        let source: SourceId = serde_json::from_str(r#""deck2.turn""#).unwrap();
        assert_eq!(source.named(), Some(&Source::Measured(1, Measure::Turn)));
    }

    #[test]
    fn a_mode_is_read_from_its_id() {
        assert_eq!(TargetId::from("mode.padsA.roll").mode(), Some(("padsA", "roll")));
        assert_eq!(TargetId::from("deck1.roll1").mode(), None);
    }
}
