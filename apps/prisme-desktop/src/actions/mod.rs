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

pub mod deck;
mod edits;
pub mod fx;
pub mod mixer;
mod perform;
pub mod sampler;
pub mod transport;
mod view;

use crate::auto_dj::{AutoDj, Takeover};
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState};
use prisme_actions::input::{Given, Input, Inputs};
use prisme_actions::lane::{lane, Lane};
use prisme_actions::perform::PerformState;
use prisme_actions::position::position;
use prisme_actions::view::View;
use prisme_control::{Kind, Owner, Target, Unknown};
use std::sync::mpsc::{self, Sender, SyncSender};
use std::sync::Mutex;
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
    perform: Mutex<PerformState>,
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
        Self { perform: Mutex::default(), inputs: Mutex::default(), slow, started: Instant::now() }
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
        Performing { state: &state, actions: self, app: Some(app), source }.perform(target, input)
    }

    /// Seconds since Prisme started.
    fn now_s(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// The application as the actions see it now.
    pub fn view(&self, app: &AppHandle) -> View {
        let auto_dj = app.try_state::<AutoDj>().is_some_and(|a| a.plays());
        view_of(&app.state(), auto_dj, &self.perform, self.now_s())
    }
}

/// An action being performed on the application's state, as `source` asks.
struct Performing<'a> {
    state: &'a AppState,
    actions: &'a Actions,
    /// What the Auto DJ and the library are reached by: none in a test.
    app: Option<&'a AppHandle>,
    source: Source<'a>,
}

impl Performing<'_> {
    /// The Auto DJ, if it runs.
    fn auto_dj(&self) -> Option<tauri::State<'_, AutoDj>> {
        self.app.and_then(|app| app.try_state::<AutoDj>())
    }

    /// The application as the actions see it now.
    fn view(&self) -> View {
        let auto_dj = self.auto_dj().is_some_and(|a| a.plays());
        view_of(self.state, auto_dj, &self.actions.perform, self.actions.now_s())
    }

    /// Seconds on the clock of the one asking: a controller's own, else Prisme's.
    fn at_s(&self) -> f64 {
        match self.source {
            Source::Controller(moved) => moved.at_s,
            Source::Interface => self.actions.now_s(),
        }
    }

    /// What `input` gives `target`: the interface's faders set what they show, a controller's
    /// pick their setting up first (see `Inputs`); nothing while it gives nothing.
    fn given(&self, target: &Target, input: Input) -> Option<Given> {
        let position = || (target.kind() == Kind::Fader).then(|| position(target, &self.view())).flatten();
        match (self.source, input) {
            (Source::Interface, Input::Value(value)) => Some(Given::Value(value)),
            // the interface's encoders under a name no controller has
            (Source::Interface, steps) => {
                let at = position();
                lock(&self.actions.inputs).resolve("", target, steps, at)
            }
            (Source::Controller(moved), input) => {
                let at = position();
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
    use prisme_engine::{Command, Engine, EngineConfig};
    use prisme_session::JsonFile;

    pub struct Bench {
        pub state: AppState,
        pub actions: Actions,
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
            Self { state, actions: Actions::start(), engine }
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
            Performing { state: &self.state, actions: &self.actions, app: None, source }
                .perform(&target, input)
        }

        /// The commands the engine was sent since last asked.
        pub fn sent(&mut self) -> Vec<Command> {
            self.engine.take_commands()
        }
    }
}
