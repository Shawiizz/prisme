//! Streams the engine state to the frontend about 60 times per second, with the sections of
//! what the interface mirrors that changed since last sent, and frees what the engine hands back
//! at the same pace. Each status says only what changed since the one before (see [`Told`]):
//! nothing at all while nothing moves. The library's rows are sent again as what they show of
//! the decks changes (see `browse::Around`), and ranked again as the deck they follow does.

use crate::actions::browse::{self, Around, RowsStems};
use crate::actions::Actions;
use crate::auto_dj::AutoDj;
use crate::dto::{ControlStateDto, StatusDto};
use crate::preview::Previews;
use crate::state::{lock, AppState, Section};
use crate::stems_service::Shown;
use prisme_actions::lane::Lane;
use prisme_actions::meter::Meters;
#[cfg(target_os = "android")]
use prisme_actions::player::Player;
use prisme_analysis::Key;
use prisme_library::browse::Column;
use serde::Serialize;
use serde_json::{Map, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

const INTERVAL: Duration = Duration::from_millis(16);

/// Incremented by every subscription: an older stream stops when it sees a newer one
/// (e.g. after a page reload).
static GENERATION: AtomicU64 = AtomicU64::new(0);

#[tauri::command]
pub fn subscribe_status(app: AppHandle, channel: Channel<Value>) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::Builder::new()
        .name("prisme-status".into())
        .spawn(move || {
            let state = app.state::<AppState>();
            let actions = app.state::<Actions>();
            let auto_dj = app.state::<AutoDj>();
            // the revisions of what the interface mirrors it was last sent: none, all sent first
            let mut told = None;
            let mut sent = Told::default();
            let mut around = Around::default();
            // the deck the library's rows are compared with, and whether they are ranked by it
            let mut compared = (0, false);
            let mut ranked_against: Option<Ranked> = None;
            let mut meters = Meters::default();
            let mut stems = Shown::default();
            #[cfg(target_os = "android")]
            let (mut player, mut lock_player, mut internal) =
                (Player::default(), crate::lock_player::LockPlayer::default(), false);
            while GENERATION.load(Ordering::SeqCst) == generation {
                // read before what it is of: a change after moves it again
                let revision = state.revision();
                let (engine, garbage) = {
                    let mut session = lock(&state.session);
                    session.as_mut().map_or((None, Vec::new()), |s| {
                        let stream = (s.stats.clone(), s.engine_config.sample_rate);
                        let engine = (s.controller.status(), s.info.layout(), stream);
                        (Some(engine), s.controller.take_garbage())
                    })
                };
                // freed, and the status built, with the session's lock released: the jogs and
                // the clock wait on it
                drop(garbage);
                if let Some(previews) = app.try_state::<Previews>() {
                    let playing = engine.as_ref().and_then(|(e, ..)| e.preview.as_ref()).map(|p| p.id);
                    previews.follow(&state, playing);
                }
                let (view, stream) = match engine {
                    Some((engine, layout, stream)) => {
                        (actions.view_with(&app, engine, Some(layout)), Some(stream))
                    }
                    None => (actions.view_with(&app, Default::default(), None), None),
                };
                let now_s = actions.now_s();
                let levels = meters.read(&view.status, now_s);
                let stream = stream.as_ref().map(|(stats, rate)| (&**stats, *rate));
                let mut status = StatusDto::of(&view, stream, levels);
                let moved = |s: Section| told.is_none_or(|t: [u64; _]| t[s as usize] != revision[s as usize]);
                status.mix_in_s = auto_dj.mix_in_s(&view.status);
                #[cfg(target_os = "android")]
                {
                    if moved(Section::Mixer) {
                        internal = state.mixes_itself();
                    }
                    if moved(Section::Tracks) || moved(Section::Settings) {
                        lock_player.read(&state);
                    }
                    lock_player.follow(&app, &view, player.read(&view, internal, now_s), now_s);
                }
                if moved(Section::Tracks) {
                    around.decks = lock(&state.tracks).each_ref().map(|t| t.as_ref().map(|t| t.path.clone()));
                }
                if moved(Section::Preview) {
                    let preview = lock(&state.preview);
                    around.previewed =
                        [preview.track.as_ref().map(|t| t.path.clone()), preview.opening.clone()];
                }
                if moved(Section::Library) {
                    let library = lock(&state.library);
                    compared = (library.browser.reference(), library.browser.sort().column == Column::Mix);
                }
                if moved(Section::Stems) || moved(Section::Settings) {
                    stems = lock(&state.stems).clone();
                    let reach = stems.reach(state.preferences.read(|p| p.library.stems_fast));
                    around.stems =
                        RowsStems { current: stems.current(), queued: stems.queued.clone(), reach };
                }
                for (deck, shown) in status.decks.iter_mut().enumerate() {
                    shown.separating = around.decks[deck].as_deref().and_then(|path| stems.separating(path));
                }
                let before = around.clone();
                around.read(&view, compared.0);
                if compared.1 {
                    rank(&app, &view, &around, compared.0, &mut ranked_against);
                }
                // what the rows show of the decks, sent again only where it shows
                let rows_moved = around != before && lock(&state.library).look_around(&around);
                if told != Some(revision) || rows_moved {
                    let sections = Section::ALL
                        .into_iter()
                        .filter(|&s| moved(s) || (rows_moved && s == Section::Library));
                    status.controls = Some(ControlStateDto::of(&state, &view, sections));
                    told = Some(revision);
                }
                if sent.changes(status).is_some_and(|changes| channel.send(changes).is_err()) {
                    break; // the window is gone
                }
                std::thread::sleep(INTERVAL);
            }
        })
        .expect("cannot start the status thread");
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
    /// that changed (the others `null`); of the sections it carries, those that changed, and of
    /// the mixer its fields that changed, its decks likewise. Nothing when nothing changed.
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

/// What the library's rows were last ranked against: the deck, its track and that track's tempo
/// and key, and the deck beside it, which the next track goes on.
type Ranked = (usize, Option<PathBuf>, Option<f64>, Option<Key>, f64, bool);

/// The library's rows ranked again against the deck `reference` if another track came on it,
/// its analysis did, or the deck beside it changed its range or its key lock: not as its tempo
/// and key move with the platter. In turn with the library's actions, not on the stream's
/// thread: a library of tens of thousands takes a while to sort.
fn rank(
    app: &AppHandle,
    view: &prisme_actions::view::View,
    around: &Around,
    reference: usize,
    last: &mut Option<Ranked>,
) {
    let deck = browse::effective(reference, view.decks_in_use);
    let track = view.decks[deck].track;
    let next = &view.decks[deck ^ 1];
    let now = (
        deck,
        around.decks[deck].clone(),
        track.and_then(|t| t.bpm),
        track.and_then(|t| t.key),
        next.tempo_range,
        next.key_lock,
    );
    if last.as_ref() != Some(&now) {
        let mix = browse::mix_reference(view, deck, around.decks[deck].as_deref());
        let ranking = app.clone();
        let job = move || ranking.state::<AppState>().browse(|l| l.browser.set_mix(mix));
        app.state::<Actions>().in_turn_on(Lane::Library, job);
        *last = Some(now);
    }
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
    fn a_status_tells_what_changed_and_nothing_while_nothing_does() {
        let state = AppState::new(JsonFile::in_memory(Default::default()));
        state.update_settings(|p| p.dj.deck_count = DeckCount::Four);
        let mut told = Told::default();
        let mut engine = EngineStatus::default();
        let first = told.changes(status(&state, engine, &Section::ALL)).unwrap();
        assert_eq!(first["decks"].as_array().map(Vec::len), Some(4));
        assert!(first["controls"]["mixer"]["decks"][3].is_object(), "all of it, the first time");
        assert_eq!(told.changes(status(&state, engine, &[])), None, "nothing moved");
        assert_eq!(told.changes(status(&state, engine, &Section::ALL)), None, "nor changed");
        engine.decks[1].position_s = 12.0;
        lock(&state.decks)[2].volume = 0.5;
        let moved = told.changes(status(&state, engine, &[Section::Mixer])).unwrap();
        let decks = moved["decks"].as_array().unwrap();
        assert!(decks[0].is_null() && decks[1]["positionS"] == 12.0, "{decks:?}");
        let mixer = moved["controls"]["mixer"].as_object().unwrap();
        assert!(mixer["decks"][2].is_object() && mixer["decks"][0].is_null());
        assert!(!mixer.contains_key("knobs") && !moved.as_object().unwrap().contains_key("meters"));
        assert!(serde_json::to_string(&moved).unwrap().len() < 8 * 1024, "Tauri's quick way in");
    }
}
