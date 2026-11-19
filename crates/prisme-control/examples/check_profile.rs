//! Reads controller profiles as Prisme does, MIDI and HID, and tells what of each it would
//! ignore or could not use.
//!
//! `cargo run -p prisme-control --example check_profile -- <profile.json>...`

use prisme_control::Profile;
use serde_json::Value;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut failed = false;
    for path in std::env::args().skip(1) {
        match check(&path) {
            Ok(summary) => println!("{path}: {summary}"),
            Err(e) => {
                failed = true;
                eprintln!("{path}: {e}");
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn check(path: &str) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let written: Value = serde_json::from_str(&text).map_err(|e| format!("not JSON: {e}"))?;
    let profile: Profile =
        serde_json::from_value(written.clone()).map_err(|e| format!("not a profile: {e}"))?;
    let read = serde_json::to_value(&profile).map_err(|e| e.to_string())?;
    let mut dropped = Vec::new();
    dropped_keys(&written, &read, &mut Vec::new(), &mut dropped);
    let ignored: Vec<String> = dropped
        .iter()
        .filter(|at| ignored(&written, &profile, at))
        .map(|at| at.iter().map(Step::to_string).collect())
        .collect();
    if !ignored.is_empty() {
        return Err(format!("keys Prisme ignores: {}", ignored.join(", ")));
    }
    if (profile.ports.is_empty() && profile.hid.is_none()) || profile.controls.is_empty() {
        return Err("names no port nor HID device, or maps nothing".into());
    }
    let problems = profile.problems();
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    let hid = profile.hid.as_ref().map_or(String::new(), |device| {
        format!(", {} fields read, {} keys lit by HID", profile.hid_inputs().len(), device.leds.len())
    });
    Ok(format!(
        "{} controls, {} lights, {} meters, {} mode groups, {} mappings{hid}",
        profile.controls.len(),
        profile.lights.len(),
        profile.meters.len(),
        profile.modes.len(),
        profile.mappings("device").count()
    ))
}

/// A step down into a JSON value.
#[derive(Clone)]
enum Step {
    Key(String),
    Index(usize),
}

impl std::fmt::Display for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Step::Key(key) => write!(f, ".{key}"),
            Step::Index(i) => write!(f, "[{i}]"),
        }
    }
}

/// Where `written` has keys that `read`, the profile written back, has not: misspelt or not
/// of the format, or left out as they hold the default.
fn dropped_keys(written: &Value, read: &Value, at: &mut Vec<Step>, out: &mut Vec<Vec<Step>>) {
    match (written, read) {
        (Value::Object(w), Value::Object(r)) => {
            for (key, value) in w {
                at.push(Step::Key(key.clone()));
                match r.get(key) {
                    Some(kept) => dropped_keys(value, kept, at, out),
                    None => out.push(at.clone()),
                }
                at.pop();
            }
        }
        (Value::Array(w), Value::Array(r)) => {
            for (i, (value, kept)) in w.iter().zip(r).enumerate() {
                at.push(Step::Index(i));
                dropped_keys(value, kept, at, out);
                at.pop();
            }
        }
        _ => {}
    }
}

/// Whether the key at `at` is ignored: another value there reads the same profile (a key of
/// the format holding its default reads another, or does not read).
fn ignored(written: &Value, profile: &Profile, at: &[Step]) -> bool {
    let mut changed = written.clone();
    let mut value = &mut changed;
    for step in at {
        value = match step {
            Step::Key(key) => &mut value[key.as_str()],
            Step::Index(i) => &mut value[*i],
        };
    }
    *value = match value {
        Value::Bool(b) => Value::Bool(!*b),
        Value::Number(n) => Value::from(n.as_f64().unwrap_or(0.0) + 1.0),
        Value::String(s) => Value::String(format!("{s}x")),
        _ => Value::String("x".into()),
    };
    serde_json::from_value::<Profile>(changed).is_ok_and(|other| &other == profile)
}
