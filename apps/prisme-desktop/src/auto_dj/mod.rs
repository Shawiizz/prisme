//! The Auto DJ: plays the prepare list one track after the other, each mixed into the next
//! with the software mixer's crossfader, on decks A and B. It runs here rather than in the
//! page so that its timing holds while the window is hidden or the phone's screen is off.
//!
//! * `plan`: where a mix starts, where the next track starts, at what speed, for how long;
//! * `machine`: what to do next, from what the decks do and what heads the list.
//!
//! A track leaves the prepare list as its mix starts. The DJ playing, stopping or moving deck
//! A or B, the crossfader, or loading either deck by hand pauses it (see [`Takeover`]).

pub mod machine;
pub mod plan;

use crate::actions::Actions;
use crate::dto::AutoDjStatusDto;
use crate::history::PREPARE_CHANGED_EVENT;
use crate::library_service::LibraryService;
use crate::preferences::AutoDjPreferences;
use crate::state::{lock, AppState};
use crate::track_loading;
use crate::track_waveform::TrackWaveform;
pub use machine::Takeover;
use machine::{Action, DeckView, Machine, TrackView, View, DECKS};
use plan::{Audible, FadeLength};
use prisme_actions::lane::Lane;
use prisme_engine::{Command, MixMode};
use prisme_library::CueSlot;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tracing::warn;

pub const AUTO_DJ_EVENT: &str = "auto-dj";
/// How often the decks are looked at: a mix starts at most this late (and is caught up).
const TICK: Duration = Duration::from_millis(10);
/// How often the first track of the prepare list is read again (the DJ reorders it).
const LIST_POLL: Duration = Duration::from_millis(250);

pub struct AutoDj {
    app: AppHandle,
    machine: Mutex<Machine>,
    /// Wakes the loop when the Auto DJ is turned on.
    wake: Condvar,
    /// The status last told to the interface.
    told: Mutex<Option<AutoDjStatusDto>>,
}

impl AutoDj {
    pub fn new(app: AppHandle, preferences: AutoDjPreferences) -> Self {
        Self {
            app,
            machine: Mutex::new(Machine::new(fade_of(preferences))),
            wake: Condvar::new(),
            told: Mutex::new(None),
        }
    }

    pub fn status(&self) -> AutoDjStatusDto {
        let machine = lock(&self.machine);
        AutoDjStatusDto::of(machine.status(), machine.fade())
    }

    /// Whether it plays or mixes.
    pub fn plays(&self) -> bool {
        matches!(lock(&self.machine).status().state, machine::State::On | machine::State::Mixing)
    }

    pub fn toggle(&self) {
        self.change(Machine::toggle);
    }

    pub fn fade_now(&self) {
        self.change(Machine::fade_now);
    }

    pub fn skip(&self) {
        self.change(Machine::skip);
    }

    pub fn set_fade(&self, preferences: AutoDjPreferences) {
        self.change(|m| m.set_fade(fade_of(preferences)));
    }

    /// The DJ acted by hand: the Auto DJ pauses if it was playing. Called at each move of a
    /// jog, the crossfader, a deck's place: nothing is woken nor told unless it paused.
    pub fn take_over(&self, takeover: Takeover) {
        let paused = {
            let mut machine = lock(&self.machine);
            let before = machine.status();
            machine.take_over(takeover);
            machine.status() != before
        };
        if paused {
            self.wake.notify_all();
            self.tell();
        }
    }

    fn change(&self, change: impl FnOnce(&mut Machine)) {
        change(&mut lock(&self.machine));
        self.wake.notify_all();
        self.tell();
    }

    /// Tells the interface the status, if it changed (the lock let go first).
    fn tell(&self) {
        let status = self.status();
        let changed = {
            let mut told = lock(&self.told);
            let changed = told.as_ref() != Some(&status);
            *told = Some(status.clone());
            changed
        };
        if changed {
            let _ = self.app.emit(AUTO_DJ_EVENT, &status);
        }
    }

    /// Runs the Auto DJ for the whole life of the app, asleep while it is off.
    pub fn start(app: AppHandle) {
        std::thread::Builder::new()
            .name("prisme-auto-dj".into())
            .spawn(move || {
                let auto_dj = app.state::<AutoDj>();
                let mut decks = Decks::default();
                let mut list = ListHead::default();
                loop {
                    drop(
                        auto_dj
                            .wake
                            .wait_while(lock(&auto_dj.machine), |m| !m.is_on())
                            .unwrap_or_else(|e| e.into_inner()),
                    );
                    if let Some(view) = decks.view(&app, list.next(&app)) {
                        let actions = lock(&auto_dj.machine).tick(&view);
                        for action in actions {
                            if matches!(action, Action::TakeOut(_)) {
                                list.forget();
                            }
                            auto_dj.perform(action);
                        }
                    }
                    auto_dj.tell();
                    std::thread::sleep(TICK);
                }
            })
            .expect("cannot start the Auto DJ thread");
    }

    fn perform(&self, action: Action) {
        let app = &self.app;
        let state = app.state::<AppState>();
        let sent = match action {
            // in turn with the DJ's actions on the deck
            Action::Load { deck, path } => {
                let app = app.clone();
                self.app.state::<Actions>().in_turn_on(Lane::Track(deck), move || {
                    let loaded = match track_loading::load_on_deck(&app, deck, path.clone()) {
                        // or the DJ's own load goes there: not the track's fault
                        Ok(_) => true,
                        Err(e) => {
                            warn!("Auto DJ: {} not loaded: {e}", path.display());
                            false
                        }
                    };
                    let auto_dj = app.state::<AutoDj>();
                    auto_dj.change(|m| m.loaded(deck, &path, loaded));
                });
                Ok(())
            }
            Action::Cue { deck, position_s } => state.send(Command::Seek { deck, seconds: position_s }),
            Action::Start { deck, from_s, rate, snap } => {
                lock(&state.decks)[deck].pitch = rate;
                let snap = snap.then_some(Command::SyncOnce { deck });
                [
                    Some(Command::SetSyncLocked { deck, locked: false }),
                    Some(Command::SetPitch { deck, ratio: rate }),
                    Some(Command::Seek { deck, seconds: from_s }),
                    snap,
                    Some(Command::SetPlaying { deck, playing: true }),
                ]
                .into_iter()
                .flatten()
                .try_for_each(|c| state.send(c))
            }
            Action::Stop { deck } => state.send(Command::SetPlaying { deck, playing: false }),
            Action::Crossfader(position) => {
                *lock(&state.crossfader) = position;
                state.send(Command::SetCrossfader { position })
            }
            Action::TakeOut(path) => {
                let library = app.state::<Arc<LibraryService>>();
                match library.db.remove_from_prepare(&[path.as_path()]) {
                    Ok(_) => {
                        let _ = app.emit(PREPARE_CHANGED_EVENT, ());
                    }
                    Err(e) => warn!("Auto DJ: {} left in the prepare list: {e}", path.display()),
                }
                Ok(())
            }
        };
        if let Err(e) = sent {
            warn!("Auto DJ: {e}");
        }
    }
}

fn fade_of(preferences: AutoDjPreferences) -> FadeLength {
    let p = preferences.bounded();
    FadeLength { beats: p.fade_beats, seconds: p.fade_s }
}

/// The first track of the prepare list, read again now and then.
#[derive(Default)]
struct ListHead {
    next: Option<PathBuf>,
    read_at: Option<Instant>,
}

impl ListHead {
    fn next(&mut self, app: &AppHandle) -> Option<PathBuf> {
        if self.read_at.is_none_or(|at| at.elapsed() >= LIST_POLL) {
            let library = app.state::<Arc<LibraryService>>();
            match library.db.prepare_list() {
                Ok(list) => self.next = list.into_iter().next().map(|e| PathBuf::from(e.path)),
                Err(e) => warn!("Auto DJ: prepare list not read: {e}"),
            }
            self.read_at = Some(Instant::now());
        }
        self.next.clone()
    }

    /// The list changed: read it again at the next look.
    fn forget(&mut self) {
        self.read_at = None;
    }
}

/// What the decks hold, where each track is heard kept for its load (from its waveform).
#[derive(Default)]
struct Decks {
    audible: [Option<(u64, Audible)>; DECKS],
}

impl Decks {
    /// The decks and the mixer as they are; `None` while the audio is not running.
    fn view(&mut self, app: &AppHandle, next: Option<PathBuf>) -> Option<View> {
        let state = app.state::<AppState>();
        let (status, info) = {
            let mut session = lock(&state.session);
            let session = session.as_mut()?;
            (session.controller.status(), session.info.clone())
        };
        let software_mixer = state.routing(&info).mode == MixMode::Internal;
        let settings = *lock(&state.decks);
        let tracks = lock(&state.tracks);
        let decks = std::array::from_fn(|d| {
            let deck = &status.decks[d];
            let track = tracks[d].as_ref().map(|t| TrackView {
                path: t.path.clone(),
                load_id: t.load_id,
                grid: t.grid(),
                cue_s: t.cue(CueSlot::Main).map(|c| c.position_s),
                audible: self.audible(d, t.load_id, &t.waveform),
            });
            DeckView {
                playing: deck.loaded && deck.playing,
                position_s: deck.position_s,
                duration_s: deck.duration_s,
                speed: deck.speed,
                bpm: deck.bpm,
                leader: deck.sync.leader,
                free: !deck.vinyl.enabled && !settings[d].passthrough,
                track,
            }
        });
        let crossfader = *lock(&state.crossfader);
        Some(View { decks, crossfader, software_mixer, next })
    }

    fn audible(&mut self, deck: usize, load_id: u64, waveform: &TrackWaveform) -> Option<Audible> {
        match self.audible[deck] {
            Some((id, audible)) if id == load_id => Some(audible),
            _ => {
                let audible = waveform.read_whole(|w| plan::audible(&w.peak, w.slices_per_second))??;
                self.audible[deck] = Some((load_id, audible));
                Some(audible)
            }
        }
    }
}
