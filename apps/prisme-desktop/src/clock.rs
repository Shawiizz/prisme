//! The backend's clock: about 60 times a second, the application as the actions see it (a
//! `View`) is built once, from the engine's state taken with what it hands back to be freed, and
//! what follows it runs, whether a page listens or not: the previews follow the engine, the keys
//! held fire (the interface's and the controllers'), the controllers show what it shows, the
//! library's rows look at the decks and are ranked against the deck they follow, a phone's lock
//! screen is told; then the page listening, if one does, is told what changed
//! (`status_stream`).

use crate::actions::browse::{self, Around, RowsStems};
use crate::actions::Actions;
use crate::auto_dj::AutoDj;
use crate::controllers::{Controllers, Levels, METERS_EVERY};
use crate::dto::StatusDto;
use crate::preview::Previews;
use crate::state::{lock, AppState, Section, SECTIONS};
use crate::status_stream::StatusStream;
use crate::stems_service::Shown;
use prisme_actions::lane::Lane;
use prisme_actions::meter::Meters;
use prisme_actions::view::View;
use prisme_analysis::Key;
use prisme_library::browse::Column;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tracing::error;

const TICK: Duration = Duration::from_millis(16);

/// Runs the clock for the whole life of the application.
pub fn start(app: AppHandle) {
    std::thread::Builder::new()
        .name("prisme-clock".into())
        .spawn(move || {
            let mut clock = Clock::default();
            loop {
                piece("tick", || clock.tick(&app));
                std::thread::sleep(TICK);
            }
        })
        .expect("cannot start the clock");
}

/// Does `work`, a piece of a tick: one that panics is logged, and what follows it goes on (the
/// controllers' lights and holds, the stream, the lock screen), this tick and the next.
fn piece(what: &str, work: impl FnOnce()) {
    if catch_unwind(AssertUnwindSafe(work)).is_err() {
        error!("the clock's {what} failed");
    }
}

/// What the library's rows were last ranked against.
#[derive(Debug, Clone, PartialEq)]
struct Ranked {
    deck: usize,
    track: Option<PathBuf>,
    bpm: Option<f64>,
    key: Option<Key>,
    /// The deck beside it, which the next track goes on: its tempo fader's range and its key lock.
    next_range: f64,
    next_key_lock: bool,
}

#[derive(Default)]
struct Clock {
    /// The revisions of what the interface mirrors when last looked at: none, all looked at first.
    seen: Option<[u64; SECTIONS]>,
    /// What the library's rows show of the decks.
    around: Around,
    /// The deck the library's rows are compared with, and whether they are ranked by it.
    compared: (usize, bool),
    ranked: Option<Ranked>,
    stems: Shown,
    /// The separations of the library's rows are looked for on the library's lane.
    looking: Arc<AtomicBool>,
    meters: Meters,
    /// When the controllers' meters were last given their levels.
    metered: Option<Instant>,
    #[cfg(target_os = "android")]
    player: Player,
}

/// The separations of the rows being looked for: over as it is dropped, however the look ended.
struct Looking(Arc<AtomicBool>);

impl Drop for Looking {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// A phone's lock screen: the deck heard, and what it is told.
#[cfg(target_os = "android")]
#[derive(Default)]
struct Player {
    heard: prisme_actions::player::Player,
    told: crate::lock_player::LockPlayer,
    /// The software mixer mixes.
    internal: bool,
}

impl Clock {
    fn tick(&mut self, app: &AppHandle) {
        let state = app.state::<AppState>();
        let actions = app.state::<Actions>();
        // read before what it is of: a change after moves it again
        let revision = state.revision();
        let seen = self.seen;
        let moved = |s: Section| seen.is_none_or(|t| t[s as usize] != revision[s as usize]);
        let (engine, garbage) = {
            let mut session = lock(&state.session);
            session.as_mut().map_or((None, Vec::new()), |s| {
                let stream = (s.stats.clone(), s.engine_config.sample_rate);
                (Some((s.controller.status(), s.info.layout(), stream)), s.controller.take_garbage())
            })
        };
        // freed, and the view built, with the session's lock released: the jogs and the clock
        // wait on it
        drop(garbage);
        // before the view: the controllers show what their holds did at once
        piece("holds", || app.state::<Controllers>().fire_holds(app));
        if let Some(previews) = app.try_state::<Previews>() {
            let playing = engine.as_ref().and_then(|(e, ..)| e.preview.as_ref()).map(|p| p.id);
            piece("preview", || previews.follow(&state, playing));
        }
        let (view, stream) = match engine {
            Some((engine, layout, stream)) => (actions.view_with(app, engine, Some(layout)), Some(stream)),
            None => (actions.view_with(app, Default::default(), None), None),
        };
        let now_s = actions.now_s();
        let due = self.metered.is_none_or(|at| at.elapsed() >= METERS_EVERY);
        let levels = (due && stream.is_some()).then(|| Levels::of(&view.status));
        if due {
            self.metered = Some(Instant::now());
        }
        piece("controllers", || app.state::<Controllers>().tick(&view, levels.as_ref(), now_s));
        // the interface's keys held, on the same clock as the controllers' holds
        piece("keys", || actions.fire_keys(app, &view));
        #[cfg(target_os = "android")]
        piece("lock player", || {
            if moved(Section::Routing) {
                self.player.internal = state.mixes_itself();
            }
            if moved(Section::Tracks) || moved(Section::Settings) {
                self.player.told.read(&state);
            }
            let heard = self.player.heard.read(&view, self.player.internal, now_s);
            self.player.told.follow(app, &view, heard, now_s);
        });
        let rows_moved = self.look_around(app, &state, &view, &moved);
        // the rows sent once their separations are known: held back until then
        let mut told = revision;
        let waits = (moved(Section::Library) || rows_moved) && self.rows_wait(app, &state);
        if waits {
            told[Section::Library as usize] = seen.map_or(0, |s| s[Section::Library as usize]);
        }
        let rows_moved = rows_moved && !waits;
        let stream_stats = stream.as_ref().map(|(stats, rate)| (&**stats, *rate));
        let stems = &self.stems;
        let around = &self.around;
        let meters = &mut self.meters;
        app.state::<StatusStream>().tell(&state, &view, told, rows_moved, || {
            let mut status = StatusDto::of(&view, stream_stats, meters.read(&view.status, now_s));
            status.mix_in_s = app.state::<AutoDj>().mix_in_s(&view.status);
            for (deck, shown) in status.decks.iter_mut().enumerate() {
                shown.separating = around.decks[deck].as_deref().and_then(|path| stems.separating(path));
            }
            status
        });
        self.seen = Some(told);
    }

    /// Whether the rows of the library's window wait for their separations, then looked for
    /// off the clock and off the lane of the library's actions (a drive may be slow): the
    /// library moves once they are known.
    fn rows_wait(&self, app: &AppHandle, state: &AppState) -> bool {
        let unknown = lock(&state.library).unknown();
        if unknown.is_empty() {
            return false;
        }
        if !self.looking.swap(true, Ordering::AcqRel) {
            let (looked, looking) = (app.clone(), Looking(self.looking.clone()));
            let job = move || {
                let state = looked.state::<AppState>();
                browse::look_for_separations(&state, unknown);
                drop(looking);
                state.moved(Section::Library);
            };
            app.state::<Actions>().in_turn_on(Lane::Rows, job);
        }
        true
    }

    /// The library's rows look at the decks, and are ranked again against the deck they follow:
    /// whether what they show of the decks changed.
    fn look_around(
        &mut self,
        app: &AppHandle,
        state: &AppState,
        view: &View,
        moved: &impl Fn(Section) -> bool,
    ) -> bool {
        if moved(Section::Tracks) {
            self.around.decks = lock(&state.tracks).each_ref().map(|t| t.as_ref().map(|t| t.path.clone()));
        }
        if moved(Section::Preview) {
            let preview = lock(&state.preview);
            self.around.previewed = [preview.track.as_ref().map(|t| t.path.clone()), preview.opening.clone()];
        }
        if moved(Section::Library) {
            let library = lock(&state.library);
            self.compared = (library.browser.reference(), library.browser.sort().column == Column::Mix);
        }
        if moved(Section::Stems) || moved(Section::Settings) {
            self.stems = lock(&state.stems).clone();
            let reach = self.stems.reach(state.preferences.read(|p| p.library.stems_fast));
            self.around.stems =
                RowsStems { current: self.stems.current(), queued: self.stems.queued.clone(), reach };
        }
        let before = self.around.clone();
        self.around.read(view, self.compared.0);
        if self.compared.1 {
            self.rank(app, view);
        }
        // what the rows show of the decks, sent again only where it shows
        self.around != before && lock(&state.library).look_around(&self.around)
    }

    /// The library's rows ranked again against the deck they follow if another track came on
    /// it, its analysis did, or the deck beside it changed its range or its key lock: not as its
    /// tempo and key move with the platter. Off the clock, and off the lane of the library's
    /// actions: a library of tens of thousands takes a while to sort.
    fn rank(&mut self, app: &AppHandle, view: &View) {
        let deck = browse::effective(self.compared.0, view.decks_in_use);
        let track = view.decks[deck].track;
        let next = &view.decks[deck ^ 1];
        let now = Ranked {
            deck,
            track: self.around.decks[deck].clone(),
            bpm: track.and_then(|t| t.bpm),
            key: track.and_then(|t| t.key),
            next_range: next.tempo_range,
            next_key_lock: next.key_lock,
        };
        if self.ranked.as_ref() != Some(&now) {
            let mix = browse::mix_reference(view, deck, self.around.decks[deck].as_deref());
            let ranking = app.clone();
            let job = move || ranking.state::<AppState>().browse(|l| l.browser.set_mix(mix));
            app.state::<Actions>().in_turn_on(Lane::Rows, job);
            self.ranked = Some(now);
        }
    }
}
