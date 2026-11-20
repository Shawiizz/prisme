//! Streams the engine state to the frontend about 60 times per second, with the sections of
//! what the interface mirrors that changed since last sent, and frees what the engine hands back
//! at the same pace. The library's rows are sent again as what they show of the decks changes
//! (see `browse::Around`), and ranked again as the deck they follow does.

use crate::actions::browse::{self, Around, RowsStems};
use crate::actions::Actions;
use crate::dto::{ControlStateDto, StatusDto};
use crate::preview::Previews;
use crate::state::{lock, AppState, Section};
use crate::stems_service::Shown;
use prisme_actions::meter::Meters;
use prisme_analysis::Key;
use prisme_library::browse::Column;
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
pub fn subscribe_status(app: AppHandle, channel: Channel<StatusDto>) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::Builder::new()
        .name("prisme-status".into())
        .spawn(move || {
            let state = app.state::<AppState>();
            let actions = app.state::<Actions>();
            // the revisions of what the interface mirrors it was last sent: none, all sent first
            let mut told = None;
            let mut around = Around::default();
            // the deck the library's rows are compared with, and whether they are ranked by it
            let mut compared = (0, false);
            let mut ranked_against: Option<Ranked> = None;
            let mut meters = Meters::default();
            let mut stems = Shown::default();
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
                let levels = meters.read(&view.status, actions.now_s());
                let stream = stream.as_ref().map(|(stats, rate)| (&**stats, *rate));
                let mut status = StatusDto::of(&view, stream, levels);
                let moved = |s: Section| told.is_none_or(|t: [u64; _]| t[s as usize] != revision[s as usize]);
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
                    rank(&state, &view, &around, compared.0, &mut ranked_against);
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
                if channel.send(status).is_err() {
                    break; // the window is gone
                }
                std::thread::sleep(INTERVAL);
            }
        })
        .expect("cannot start the status thread");
}

/// What the library's rows were last ranked against: the deck, its track and that track's tempo
/// and key, and the deck beside it, which the next track goes on.
type Ranked = (usize, Option<PathBuf>, Option<f64>, Option<Key>, f64, bool);

/// The library's rows ranked again against the deck `reference` if another track came on it,
/// its analysis did, or the deck beside it changed its range or its key lock: not as its tempo
/// and key move with the platter.
fn rank(
    state: &AppState,
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
        state.browse(|l| l.browser.set_mix(mix));
        *last = Some(now);
    }
}
