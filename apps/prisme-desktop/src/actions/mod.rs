//! The actions of the application, with one way in for all who ask for them
//! ([`Actions::perform`]): the controllers from their threads, and the interface
//! ([`perform_action`]). An action driving the engine or what is kept in memory is done at once
//! on the thread asking for it; one reading or writing the disk or the library, in turn on a
//! worker of its own (`prisme_actions::lane`), so that a press and its release keep their
//! order. What each action does is in `perform`; the zones' modules hold what the actions and
//! the interface's commands both do, done once. What the actions keep between presses is here,
//! and the view of the application the controllers' lights and values are read from
//! ([`Actions::view`]). The actions the interface performs yet, a controller tells it
//! (`ui-action`).

mod cues;
pub mod deck;
mod edits;
pub mod fx;
mod general;
mod loops;
pub mod mixer;
pub mod pads;
mod perform;
pub mod sampler;
pub mod transport;
mod view;

use crate::auto_dj::{AutoDj, Takeover};
use crate::error::{AppError, AppResult};
use crate::library_service::LibraryService;
use crate::state::{lock, AppState, MirroredGuard};
use prisme_actions::input::{Given, Input, Inputs};
use prisme_actions::lane::{lane, Lane};
use prisme_actions::perform::{Clips, PerformState};
use prisme_actions::position::position;
use prisme_actions::view::View;
use prisme_control::{Owner, Target, Unknown};
use prisme_library::LibraryDb;
use std::sync::mpsc::{self, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};

pub use view::view_of;

/// A failure of an action a controller asked for, for the interface to word (an `AppError`).
pub const FAILED_EVENT: &str = "action-failed";

/// A move of a controller: which one, when on its clock, and how many steps a turn of its jog
/// wheels sends.
#[derive(Debug, Clone, Copy)]
pub struct Moved<'d> {
    pub device: &'d str,
    pub at_s: f64,
    pub jog_steps_per_turn: f64,
}

/// Who asks for an action.
#[derive(Debug, Clone, Copy)]
pub enum Source<'d> {
    /// A controller's control moved.
    Controller(Moved<'d>),
    /// The interface: a click, a drag, a key, the command palette.
    Interface,
}

/// A [`Source`] kept for the worker.
enum Asked {
    Controller { device: String, at_s: f64, jog_steps_per_turn: f64 },
    Interface,
}

impl Asked {
    fn of(source: Source) -> Self {
        match source {
            Source::Controller(Moved { device, at_s, jog_steps_per_turn }) => {
                Asked::Controller { device: device.to_owned(), at_s, jog_steps_per_turn }
            }
            Source::Interface => Asked::Interface,
        }
    }

    fn source(&self) -> Source<'_> {
        match self {
            Asked::Controller { device, at_s, jog_steps_per_turn } => {
                Source::Controller(Moved { device, at_s: *at_s, jog_steps_per_turn: *jog_steps_per_turn })
            }
            Asked::Interface => Source::Interface,
        }
    }
}

/// Who hears how an action went.
pub enum Answer {
    /// The interface, of a failure ([`FAILED_EVENT`]): a controller asked, and goes on.
    Told,
    /// The one asking, waiting for it.
    To(SyncSender<AppResult<()>>),
}

impl Answer {
    fn give(self, app: &AppHandle, outcome: AppResult<()>) {
        match (self, outcome) {
            (Answer::To(asking), outcome) => drop(asking.send(outcome)),
            (Answer::Told, Err(e)) => drop(app.emit(FAILED_EVENT, &e)),
            (Answer::Told, Ok(())) => {}
        }
    }
}

type Job = Box<dyn FnOnce() + Send>;

pub struct Actions {
    /// When the levels last clipped, for the lights.
    clips: Mutex<Clips>,
    /// The controllers' faders picking their settings up, and their encoders' steps short of
    /// one.
    inputs: Mutex<Inputs>,
    /// The worker's queue (see [`Lane::Slow`]).
    slow: Sender<Job>,
    /// What the actions are timed by when no controller's clock tells it.
    started: Instant,
}

impl Actions {
    /// The actions, the worker of the slow lane started.
    pub fn start() -> Self {
        let (slow, jobs) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("prisme-actions".into())
            .spawn(move || jobs.into_iter().for_each(|job| job()))
            .expect("cannot start the actions' worker");
        Self { clips: Mutex::default(), inputs: Mutex::default(), slow, started: Instant::now() }
    }

    /// Performs `target` driven by `input`, as `source` asks: at once here, or in turn on the
    /// worker; `answer` hears how it went. Never called from the audio thread.
    pub fn perform(&self, app: &AppHandle, target: &Target, input: Input, source: Source, answer: Answer) {
        match lane(target) {
            Lane::Instant => answer.give(app, self.perform_here(app, target, input, source)),
            Lane::Slow => {
                let (app, target, asked) = (app.clone(), target.clone(), Asked::of(source));
                let job = move || {
                    let outcome = app.state::<Actions>().perform_here(&app, &target, input, asked.source());
                    answer.give(&app, outcome);
                };
                // the worker lives as long as the application
                let _ = self.slow.send(Box::new(job));
            }
        }
    }

    fn perform_here(&self, app: &AppHandle, target: &Target, input: Input, source: Source) -> AppResult<()> {
        if target.owner() == Owner::Ui {
            return Err(not_performed(target));
        }
        let state = app.state::<AppState>();
        let library = app.try_state::<Arc<LibraryService>>();
        let library = library.as_ref().map(|l| &*l.db);
        Performing { state: &state, actions: self, app: Some(app), library, source }.perform(target, input)
    }

    /// Seconds since Prisme started.
    fn now_s(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// The application as the actions see it now.
    pub fn view(&self, app: &AppHandle) -> View {
        let auto_dj = app.try_state::<AutoDj>().is_some_and(|a| a.plays());
        view_of(&app.state(), auto_dj, &self.clips, self.now_s())
    }
}

/// An action being performed on the application's state, as `source` asks.
struct Performing<'a> {
    state: &'a AppState,
    actions: &'a Actions,
    /// What the Auto DJ and the library's edits are reached by: none in a test.
    app: Option<&'a AppHandle>,
    /// Where the cues are kept, once open.
    library: Option<&'a LibraryDb>,
    source: Source<'a>,
}

impl Performing<'_> {
    /// The Auto DJ, if it runs.
    fn auto_dj(&self) -> Option<tauri::State<'_, AutoDj>> {
        self.app.and_then(|app| app.try_state::<AutoDj>())
    }

    /// What the actions keep between presses.
    fn perform_state(&self) -> MirroredGuard<'_, PerformState> {
        lock(&self.state.perform)
    }

    /// The Auto DJ lets go of `deck`'s transport, the DJ taking it.
    fn take_over_transport(&self, deck: usize) {
        take_over(self.auto_dj().as_deref(), Takeover::Transport(deck));
    }

    /// The track on `deck` played or paused; the Auto DJ lets the deck go.
    fn set_playing(&self, deck: usize, playing: bool) -> AppResult<()> {
        transport::set_playing(self.state, self.auto_dj().as_deref(), deck, playing)
    }

    /// To `seconds` into the track on `deck`; the Auto DJ lets the deck go.
    fn seek(&self, deck: usize, seconds: f64) -> AppResult<()> {
        transport::seek(self.state, self.auto_dj().as_deref(), deck, seconds)
    }

    /// Seconds on the clock of the one asking: a controller's own, else Prisme's.
    fn at_s(&self) -> f64 {
        match self.source {
            Source::Controller(moved) => moved.at_s,
            Source::Interface => self.actions.now_s(),
        }
    }

    /// What `input` gives `target`: the interface's faders set what they show, a controller's
    /// pick their setting up first (see `Inputs`); nothing while it gives nothing. Only where
    /// its fader stands is read.
    fn given(&self, target: &Target, input: Input) -> Option<Given> {
        match (self.source, input) {
            (Source::Interface, Input::Value(value)) => Some(Given::Value(value)),
            // the interface's encoders under a name no controller has
            (Source::Interface, steps) => {
                let at = position(target, self);
                lock(&self.actions.inputs).resolve("", target, steps, at)
            }
            (Source::Controller(moved), input) => {
                let at = position(target, self);
                lock(&self.actions.inputs).resolve(moved.device, target, input, at)
            }
        }
    }
}

/// The Auto DJ, if there is one, lets go of what the DJ took over by hand.
pub fn take_over(auto_dj: Option<&AutoDj>, takeover: Takeover) {
    if let Some(auto_dj) = auto_dj {
        auto_dj.take_over(takeover);
    }
}

fn not_performed(target: &Target) -> AppError {
    AppError::Other(format!("{target} is the interface's to perform"))
}

/// Performs the action `target` driven to `value` (an encoder's `steps`, each moving a fader
/// by `step`), as the interface asks.
#[tauri::command]
pub async fn perform_action(
    app: AppHandle,
    target: String,
    value: f32,
    steps: bool,
    step: Option<f32>,
) -> AppResult<()> {
    let target: Target = target.parse().map_err(|e: Unknown| AppError::Other(e.to_string()))?;
    let (answer, outcome) = mpsc::sync_channel(1);
    let input = Input::of(value, steps, step);
    app.state::<Actions>().perform(&app, &target, input, Source::Interface, Answer::To(answer));
    match outcome.try_recv() {
        Ok(outcome) => outcome,
        // queued on the worker
        Err(_) => {
            crate::commands::blocking(move || {
                outcome.recv().unwrap_or_else(|e| Err(AppError::Other(e.to_string())))
            })
            .await
        }
    }
}

#[cfg(test)]
pub(crate) mod bench {
    //! An application whose engine no sound card calls: what an action sends it is read back.

    use super::*;
    use crate::audio_host::AudioSession;
    use crate::dto::{AudioInfo, BackendDto};
    use crate::state::LoadedTrack;
    use crate::track_waveform::TrackWaveform;
    use prisme_engine::{Command, Engine, EngineConfig};
    use prisme_media::TrackAudio;
    use prisme_session::JsonFile;

    pub struct Bench {
        pub state: AppState,
        pub actions: Actions,
        pub library: LibraryDb,
        engine: Engine,
    }

    impl Bench {
        /// Two decks, the audio running.
        pub fn new() -> Self {
            let config = EngineConfig::new(48_000, 2, &prisme_timecode::SERATO_CV02_A);
            let (engine, controller) = prisme_engine::create(config);
            let info = AudioInfo {
                backend: BackendDto::Asio,
                device: String::new(),
                sample_rate: 48_000,
                output_channels: 4,
                input_channels: 4,
                decks: 2,
                simulated: false,
                missing: None,
            };
            let state = AppState::new(JsonFile::in_memory(Default::default()));
            *lock(&state.session) = Some(AudioSession::idle(controller, config, info));
            let library = LibraryDb::open_in_memory().unwrap();
            Self { state, actions: Actions::start(), library, engine }
        }

        /// `id` driven to `value` by the controller `device`.
        pub fn moved(&self, device: &str, id: &str, input: Input) -> AppResult<()> {
            let source = Source::Controller(Moved { device, at_s: 0.0, jog_steps_per_turn: 720.0 });
            self.perform(id, input, source)
        }

        /// `id` driven to `value` by the controller `DDJ`.
        pub fn press(&self, id: &str, value: f32) -> AppResult<()> {
            self.moved("DDJ", id, Input::Value(value))
        }

        pub fn perform(&self, id: &str, input: Input, source: Source) -> AppResult<()> {
            let target: Target = id.parse().unwrap();
            assert_eq!(target.owner(), Owner::Rust, "{id}");
            self.performing(source).perform(&target, input)
        }

        /// An action asked for by `source`.
        pub(in crate::actions) fn performing<'a>(&'a self, source: Source<'a>) -> Performing<'a> {
            Performing {
                state: &self.state,
                actions: &self.actions,
                app: None,
                library: Some(&self.library),
                source,
            }
        }

        /// Where the fader of `id` stands.
        pub fn position(&self, id: &str) -> Option<f32> {
            let view = view_of(&self.state, false, &self.actions.clips, 0.0);
            position(&id.parse().unwrap(), &view)
        }

        /// The commands the engine was sent since last asked, not applied.
        pub fn sent(&mut self) -> Vec<Command> {
            self.engine.take_commands()
        }

        /// The engine run for `seconds`, a period of 10 ms at least: what it was sent applied,
        /// its state told.
        pub fn run(&mut self, seconds: f64) {
            let frames = 480;
            let (input, mut output) = (vec![0.0; frames * 2], vec![0.0; frames * 2]);
            for _ in 0..(seconds / 0.01).round().max(1.0) as usize {
                self.engine.process(&input, 2, &mut output, 2);
            }
        }

        /// A silent track of `seconds` on `deck`, with a beat grid at `bpm` from its start if
        /// it has one, stopped at its start.
        pub fn load(&mut self, deck: usize, seconds: f64, bpm: Option<f64>) {
            let audio = TrackAudio::from_interleaved(48_000, vec![0.0; (seconds * 48_000.0) as usize * 2]);
            let tempo = bpm.map(|bpm| prisme_analysis::Tempo { bpm, first_beat_s: 0.0, confidence: 1.0 });
            let track = LoadedTrack {
                load_id: LoadedTrack::next_load_id(),
                path: format!("deck{deck}.flac").into(),
                audio: Arc::new(audio),
                waveform: TrackWaveform::growing(1),
                detected: prisme_analysis::TrackAnalysis { tempo, key: None },
                edits: Default::default(),
                tags: Default::default(),
                cues: Vec::new(),
                key_shift: 0,
                stems: Default::default(),
                stem_mix: Default::default(),
                loudness: None,
            };
            let commands = track.load_commands(deck, false);
            let turn = self.state.ask_load(deck);
            self.state.put_track(deck, turn, Some(track), commands).unwrap();
            self.run(0.01);
        }

        /// The track on `deck` put at `seconds` on the screen, the engine run.
        pub fn at(&mut self, deck: usize, seconds: f64) {
            let duration_s = self.status(deck).duration_s;
            let input = Input::Value((seconds / duration_s) as f32);
            self.perform(&format!("deck{}.position", deck + 1), input, Source::Interface).unwrap();
            self.run(0.01);
        }

        /// What the engine shows of `deck`.
        pub fn status(&self, deck: usize) -> prisme_engine::DeckStatus {
            self.state.deck_status(deck).unwrap()
        }
    }
}
