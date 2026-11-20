//! The names of things, for the interface to show in its language.

use serde::ser::{Serialize, SerializeMap, Serializer};
use std::collections::BTreeSet;

/// A name to show: its English text, which is also the key of its translations, its `{name}`
/// placeholders filled from `params`.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub key: &'static str,
    pub params: Vec<(&'static str, Param)>,
}

/// What a placeholder of a label is filled with: a number, a text shown as it is (a deck's
/// letter, a fraction of a beat), or another label, translated too.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(untagged)]
pub enum Param {
    Number(f64),
    Text(String),
    Label(Label),
}

impl Label {
    pub const fn new(key: &'static str) -> Self {
        Label { key, params: Vec::new() }
    }

    /// The placeholder `name` filled with `value`.
    pub fn with(mut self, name: &'static str, value: impl Into<Param>) -> Self {
        self.params.push((name, value.into()));
        self
    }

    /// In English: its key, the placeholders filled.
    pub fn english(&self) -> String {
        let mut text = self.key.to_owned();
        for (name, param) in &self.params {
            let value = match param {
                Param::Number(n) => n.to_string(),
                Param::Text(text) => text.clone(),
                Param::Label(label) => label.english(),
            };
            text = text.replace(&format!("{{{name}}}"), &value);
        }
        text
    }

    /// Its key and those of the labels it is filled with.
    pub fn keys(&self, keys: &mut BTreeSet<&'static str>) {
        keys.insert(self.key);
        for (_, param) in &self.params {
            if let Param::Label(label) = param {
                label.keys(keys);
            }
        }
    }
}

impl Serialize for Label {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        struct Params<'p>(&'p [(&'static str, Param)]);
        impl Serialize for Params<'_> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_map(self.0.iter().map(|(name, param)| (name, param)))
            }
        }
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("key", self.key)?;
        if !self.params.is_empty() {
            map.serialize_entry("params", &Params(&self.params))?;
        }
        map.end()
    }
}

impl From<usize> for Param {
    fn from(n: usize) -> Self {
        Param::Number(n as f64)
    }
}

impl From<i32> for Param {
    fn from(n: i32) -> Self {
        Param::Number(n.into())
    }
}

impl From<f64> for Param {
    fn from(n: f64) -> Self {
        Param::Number(n)
    }
}

impl From<String> for Param {
    fn from(text: String) -> Self {
        Param::Text(text)
    }
}

impl From<Label> for Param {
    fn from(label: Label) -> Self {
        Param::Label(label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_label_reads_in_english_its_placeholders_filled() {
        let label = Label::new("{control}: on").with("control", Label::new("Hot cue {n}").with("n", 3));
        assert_eq!(label.english(), "Hot cue 3: on");
        assert_eq!(Label::new("Loop {n} beats").with("n", 0.5).english(), "Loop 0.5 beats");
        assert_eq!(Label::new("Jump {n} beats").with("n", -8.0).english(), "Jump -8 beats");
    }

    #[test]
    fn a_label_is_sent_as_its_key_and_its_values() {
        let label = Label::new("{control}: on").with("control", Label::new("Sync"));
        assert_eq!(
            serde_json::to_value(&label).unwrap(),
            json!({ "key": "{control}: on", "params": { "control": { "key": "Sync" } } })
        );
        let mut keys = BTreeSet::new();
        label.keys(&mut keys);
        assert_eq!(keys, BTreeSet::from(["Sync", "{control}: on"]));
    }
}
