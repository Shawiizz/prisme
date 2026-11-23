//! Tells the page the state of the application about 60 times a second, as the backend's clock
//! builds it (`clock`), with the sections of what the interface mirrors that moved since last
//! sent. Each status says only what changed since the one before (see [`Told`]): nothing at all
//! while nothing moves.

use crate::actions::Actions;
use crate::dto::{ControlStateDto, StatusDto};
use crate::state::{lock, AppState, Section, SECTIONS};
use prisme_actions::view::View;
use serde::Serialize;
use serde_json::{Map, Value};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::sync::Mutex;
use tauri::ipc::Channel;
use tauri::State;

/// The page listening, if one does.
#[derive(Default)]
pub struct StatusStream {
    listener: Mutex<Option<Listener>>,
}

struct Listener {
    channel: Channel<Value>,
    told: Told,
    /// The revisions of what the interface mirrors it was last sent: none, all sent first.
    revision: Option<[u64; SECTIONS]>,
}

/// The page listens from now on: a newer one in place of an older (a page reloaded), the keys
/// the older held let go.
#[tauri::command]
pub fn subscribe_status(
    stream: State<'_, StatusStream>,
    actions: State<'_, Actions>,
    channel: Channel<Value>,
) {
    actions.page_loaded();
    *lock(&stream.listener) = Some(Listener { channel, told: Told::default(), revision: None });
}

impl StatusStream {
    /// Tells the page listening, if one does, what changed of the status `status` builds (only
    /// then) and of the sections of `state` that moved since it was last told at `revision`,
    /// read in `view`; the library's too if its rows were sent again (`rows_moved`).
    pub fn tell(
        &self,
        state: &AppState,
        view: &View,
        revision: [u64; SECTIONS],
        rows_moved: bool,
        status: impl FnOnce() -> StatusDto,
    ) {
        // taken out while it is told: nothing waits on its lock meanwhile
        let Some(mut listener) = lock(&self.listener).take() else { return };
        let told =
            catch_unwind(AssertUnwindSafe(|| listener.tell(state, view, revision, rows_moved, status)));
        let kept = match &told {
            Ok(kept) => kept.then_some(listener),
            // what it holds no longer known: told whole next time
            Err(_) => Some(Listener { told: Told::default(), revision: None, ..listener }),
        };
        if let Some(listener) = kept {
            lock(&self.listener).get_or_insert(listener);
        }
        if let Err(panic) = told {
            resume_unwind(panic);
        }
    }
}

impl Listener {
    /// Sends what changed: whether the page still listens (its window is gone when its channel
    /// is).
    fn tell(
        &mut self,
        state: &AppState,
        view: &View,
        revision: [u64; SECTIONS],
        rows_moved: bool,
        status: impl FnOnce() -> StatusDto,
    ) -> bool {
        let mut status = status();
        if self.revision != Some(revision) || rows_moved {
            let told = self.revision;
            let moved = |s: Section| told.is_none_or(|t| t[s as usize] != revision[s as usize]);
            let sections =
                Section::ALL.into_iter().filter(|&s| moved(s) || (rows_moved && s == Section::Library));
            status.controls = Some(ControlStateDto::of(state, view, sections));
            self.revision = Some(revision);
        }
        let changes = self.told.changes(status);
        changes.is_none_or(|changes| self.channel.send(changes).is_ok())
    }
}

/// What the interface was last told, as it holds it: a status sends only what changed of it.
#[derive(Default)]
struct Told {
    status: Option<Map<String, Value>>,
    mixer: Option<Map<String, Value>>,
    /// Each section of what the interface mirrors, but the mixer, by its name.
    sections: Map<String, Value>,
}

impl Told {
    /// What to send of `status`: its fields that changed since last told, of its decks those
    /// that changed (the others `null`); of the sections it carries, those that changed, whole,
    /// but of the mixer its fields that changed. Nothing when nothing changed.
    fn changes(&mut self, mut status: StatusDto) -> Option<Value> {
        let controls = status.controls.take();
        let mut sent = changed(&mut self.status, object(&status));
        if let Some(controls) = controls {
            let mut changes = Map::new();
            for (section, now) in object(&controls) {
                if section == "mixer" {
                    let Value::Object(now) = now else { continue };
                    let mixer = changed(&mut self.mixer, now);
                    if !mixer.is_empty() {
                        changes.insert(section, Value::Object(mixer));
                    }
                } else if self.sections.get(&section) != Some(&now) {
                    self.sections.insert(section.clone(), now.clone());
                    changes.insert(section, now);
                }
            }
            if !changes.is_empty() {
                sent.insert("controls".into(), Value::Object(changes));
            }
        }
        (!sent.is_empty()).then_some(Value::Object(sent))
    }
}

/// `value`'s fields.
fn object(value: &impl Serialize) -> Map<String, Value> {
    match serde_json::to_value(value) {
        Ok(Value::Object(fields)) => fields,
        _ => Map::new(),
    }
}

/// The fields of `now` that changed since `told`, which becomes `now`: all of them the first
/// time. Of `decks`, as many as before, those that changed, the others `null`.
fn changed(told: &mut Option<Map<String, Value>>, now: Map<String, Value>) -> Map<String, Value> {
    let changes = match told {
        None => now.clone(),
        Some(before) => now
            .iter()
            .filter(|&(key, value)| before.get(key) != Some(value))
            .map(|(key, value)| {
                let value = match (key.as_str(), before.get(key), value) {
                    ("decks", Some(Value::Array(was)), Value::Array(decks)) if was.len() == decks.len() => {
                        let deck =
                            |(was, now): (&Value, &Value)| if was == now { Value::Null } else { now.clone() };
                        Value::Array(was.iter().zip(decks).map(deck).collect())
                    }
                    _ => value.clone(),
                };
                (key.clone(), value)
            })
            .collect(),
    };
    *told = Some(now);
    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preferences::DeckCount;
    use prisme_engine::EngineStatus;
    use prisme_session::JsonFile;

    /// The status of the engine at `engine`, with the `sections` of what the interface mirrors.
    fn status(state: &AppState, engine: EngineStatus, sections: &[Section]) -> StatusDto {
        let view = crate::actions::view_with(state, engine, None, false, &Default::default(), 0.0);
        let mut status = StatusDto::of(&view, None, Default::default());
        if !sections.is_empty() {
            status.controls = Some(ControlStateDto::of(state, &view, sections.iter().copied()));
        }
        status
    }

    #[test]
    fn a_page_told_of_a_status_that_failed_listens_on_and_is_told_it_whole() {
        let state = AppState::new(JsonFile::in_memory(Default::default()));
        let view =
            crate::actions::view_with(&state, EngineStatus::default(), None, false, &Default::default(), 0.0);
        let sent = std::sync::Arc::new(Mutex::new(0));
        let counted = sent.clone();
        let channel = Channel::new(move |_| {
            *lock(&counted) += 1;
            Ok(())
        });
        let stream = StatusStream::default();
        *lock(&stream.listener) = Some(Listener { channel, told: Told::default(), revision: None });
        let tell = || {
            stream.tell(&state, &view, state.revision(), false, || {
                StatusDto::of(&view, None, Default::default())
            })
        };
        tell();
        tell();
        assert_eq!(*lock(&sent), 1, "nothing moved the second time");
        let failed = || stream.tell(&state, &view, state.revision(), false, || panic!("no status"));
        assert!(catch_unwind(AssertUnwindSafe(failed)).is_err());
        tell();
        assert_eq!(*lock(&sent), 2, "listening still, told it whole");
    }

    #[test]
    fn a_status_tells_what_changed_and_nothing_while_nothing_does() {
        let state = AppState::new(JsonFile::in_memory(Default::default()));
        state.update_settings(|p| p.dj.deck_count = DeckCount::Four);
        let mut told = Told::default();
        let mut engine = EngineStatus::default();
        let first = told.changes(status(&state, engine, &Section::ALL)).unwrap();
        assert_eq!(first["decks"].as_array().map(Vec::len), Some(4));
        assert!(first["controls"]["decks"][3].is_object(), "all of it, the first time");
        assert_eq!(told.changes(status(&state, engine, &[])), None, "nothing moved");
        assert_eq!(told.changes(status(&state, engine, &Section::ALL)), None, "nor changed");
        engine.decks[1].position_s = 12.0;
        lock(&state.decks)[2].volume = 0.5;
        let moved = told.changes(status(&state, engine, &[Section::Mixer, Section::Decks])).unwrap();
        let decks = moved["decks"].as_array().unwrap();
        assert!(decks[0].is_null() && decks[1]["positionS"] == 12.0, "{decks:?}");
        let controls = moved["controls"].as_object().unwrap();
        let settings = controls["decks"].as_array().unwrap();
        assert!(settings.iter().all(Value::is_object) && settings[2]["volume"] == 0.5, "whole");
        assert!(!controls.contains_key("mixer"), "the mixer as it was");
        assert!(!moved.as_object().unwrap().contains_key("meters"));
        assert!(serde_json::to_string(&moved).unwrap().len() < 8 * 1024, "Tauri's quick way in");
    }
}
