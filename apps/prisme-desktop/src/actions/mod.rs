//! The actions of the application, with one way in for all who ask for them
//! ([`Actions::perform`]): the controllers from their threads, and the interface
//! ([`perform_action`]). An action driving the engine or what is kept in memory is done at once
//! on the thread asking for it; one reading or writing the disk or the library, in turn on a
//! worker of its own (`prisme_actions::lane`), so that a press and its release keep their
//! order. What the actions keep between presses is here, and the view of the application the
//! controllers' lights and values are read from ([`Actions::view`]).
//! The actions the interface performs yet, a controller tells it (`ui-action`).

mod deck;
mod view;

use crate::error::{AppError, AppResult};
use crate::state::lock;
use prisme_actions::input::{Given, Input, Inputs};
use prisme_actions::lane::{lane, Lane};
use prisme_actions::perform::PerformState;
use prisme_actions::view::View;
use prisme_control::{Owner, Target, Unknown};
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
        match target {
            Target::Deck(deck, action) if target.owner() == Owner::Rust => {
                deck::perform(app, self, *deck, *action, input, source)
            }
            _ => Err(AppError::Other(format!("{target} is the interface's to perform"))),
        }
    }

    /// What `input` of `source` gives `target`, whose setting stands at `position` if it is a
    /// fader's: the interface's faders set what they show, a controller's pick their setting up
    /// first (see `Inputs`).
    fn given(&self, source: Source, target: &Target, input: Input, position: Option<f32>) -> Option<Given> {
        match (source, input) {
            (Source::Interface, Input::Value(value)) => Some(Given::Value(value)),
            // the interface's encoders under a name no controller has
            (Source::Interface, steps) => lock(&self.inputs).resolve("", target, steps, position),
            (Source::Controller(moved), input) => {
                lock(&self.inputs).resolve(moved.device, target, input, position)
            }
        }
    }

    /// Seconds since Prisme started.
    fn now_s(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// The application as the actions see it now.
    pub fn view(&self, app: &AppHandle) -> View {
        let auto_dj = app.try_state::<crate::auto_dj::AutoDj>().is_some_and(|a| a.plays());
        view_of(&app.state(), auto_dj, &self.perform, self.now_s())
    }
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
mod tests {
    use super::*;
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;

    #[test]
    fn the_slow_lane_does_its_jobs_in_turn_on_one_thread() {
        let actions = Actions::start();
        let (done, order) = mpsc::channel();
        for n in 0..3 {
            let done = done.clone();
            let job = move || done.send((n, std::thread::current().name().map(str::to_owned))).unwrap();
            actions.slow.send(Box::new(job)).unwrap();
        }
        let got: Vec<_> = (0..3).map(|_| order.recv_timeout(Duration::from_secs(5)).unwrap()).collect();
        assert!(got.iter().all(|(_, thread)| thread.as_deref() == Some("prisme-actions")));
        assert_eq!(got.iter().map(|(n, _)| *n).collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!(order.recv_timeout(Duration::from_millis(10)), Err(RecvTimeoutError::Timeout));
    }
}
