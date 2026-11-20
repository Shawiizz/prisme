//! The actions of the application, with one way in for all who ask for them
//! ([`Actions::perform`]): the controllers from their threads, and the interface
//! ([`perform_action`]). Each action keeps its order among those of its lane
//! (`prisme_actions::lane`): it is done at once on the thread asking for it while its lane has
//! none waiting, else in turn by the lane's worker, as one opening a file always is; the actions
//! one move drives keep their order whatever their lanes. What an action changes of the library
//! (a cue, a beat grid) is in memory and in the engine at once, and kept in the library in turn
//! by a worker of its own ([`Actions::keep`]). What each action does is in `perform`; the zones'
//! modules hold what the actions and the interface's commands both do, done once. What the
//! actions keep between presses is here, and the view of the application the controllers'
//! lights and values are read from ([`Actions::view`]). The actions the interface performs (the
//! library's browsing, its windows), a controller tells it (`ui-action`).

mod cues;
pub mod deck;
mod edits;
pub mod fx;
pub mod general;
pub mod loads;
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
use prisme_actions::lane::{lane, reads_files, Lane};
use prisme_actions::perform::{Memory, PerformState};
use prisme_actions::position::position;
use prisme_actions::view::View;
use prisme_control::{DeckAction, LibraryAction, Owner, SampleAction, Target, Unknown};
use prisme_library::LibraryDb;
use serde::Serialize;
use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender, SyncSender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tracing::error;

pub use view::view_of;

/// A failure of an action a controller asked for, for the interface to word (an `AppError`).
pub const FAILED_EVENT: &str = "action-failed";
/// How long a load waits for the interface to apply the browsing sent before it.
const SELECTION_WAIT: Duration = Duration::from_millis(500);
/// How long an action of the interface waits for the one sent before it, which may be lost.
const ORDER_WAIT: Duration = Duration::from_millis(50);

/// A move of a controller: which one, when on its clock, and how many steps a turn of its jog
/// wheels sends. Its name is borrowed as it moves, owned once kept for a lane's worker.
#[derive(Debug, Clone)]
pub struct Moved<'d> {
    pub device: Cow<'d, str>,
    pub at_s: f64,
    pub jog_steps_per_turn: f64,
}

/// Who asks for an action.
#[derive(Debug, Clone)]
pub enum Source<'d> {
    /// A controller's control moved.
    Controller(Moved<'d>),
    /// The interface: a click, a drag, a key, the command palette; `confirmed`, the DJ having
    /// said yes to what the action replaces (the track of a deck that plays).
    Interface { confirmed: bool },
}

/// An action asked for, as it was sent, whenever it is performed: by whom, when on the clock of
/// the one asking (a controller's own, else Prisme's), and after how many of the actions the
/// interface was told to perform (`ui-action`).
#[derive(Debug, Clone)]
pub struct Asked<'d> {
    pub source: Source<'d>,
    pub at_s: f64,
    pub told: u64,
}

impl Asked<'_> {
    /// Kept for a lane's worker: the controller's name owned.
    fn into_owned(self) -> Asked<'static> {
        let source = match self.source {
            Source::Controller(moved) => {
                Source::Controller(Moved { device: Cow::Owned(moved.device.into_owned()), ..moved })
            }
            Source::Interface { confirmed } => Source::Interface { confirmed },
        };
        Asked { source, ..self }
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
    fn give(self, host: &impl Host, outcome: AppResult<()>) {
        match (self, outcome) {
            (Answer::To(asking), outcome) => drop(asking.send(outcome)),
            (Answer::Told, Err(e)) => host.tell(FAILED_EVENT, &e),
            (Answer::Told, Ok(())) => {}
        }
    }
}

/// What the actions are performed on, and what they tell: the application, or a bench in the
/// tests. Kept by the lanes' workers for the actions they perform in turn.
pub(crate) trait Host: Clone + Send + Sync + 'static {
    fn actions(&self) -> &Actions;
    fn app_state(&self) -> &AppState;
    /// Performs `target` driven by `input` as `asked`, at once on this thread.
    fn perform_here(&self, target: &Target, input: Input, asked: Asked) -> AppResult<()>;
    /// Tells the interface `event`.
    fn tell<P: Serialize + Clone>(&self, event: &str, payload: P);
    /// The application as the actions see it now.
    fn view(&self) -> View;

    /// Performs `target` driven by `input` as `source` asks, after the actions `in_turn` sent
    /// before it (see [`Actions::perform`]): the interface hears of a failure.
    fn perform(&self, target: &Target, input: Input, source: Source, in_turn: &mut InTurn) {
        self.actions().perform(self, target, input, source, Answer::Told, in_turn);
    }
}

impl Host for AppHandle {
    fn actions(&self) -> &Actions {
        self.state::<Actions>().inner()
    }

    fn app_state(&self) -> &AppState {
        self.state::<AppState>().inner()
    }

    fn perform_here(&self, target: &Target, input: Input, asked: Asked) -> AppResult<()> {
        if target.owner() == Owner::Ui {
            return Err(AppError::NotPerformed(target.to_string()));
        }
        let library = self.try_state::<Arc<LibraryService>>();
        let library = library.as_ref().map(|l| &l.db);
        let (state, actions) = (self.app_state(), self.actions());
        Performing { state, actions, app: Some(self), library, asked }.perform(target, input)
    }

    fn tell<P: Serialize + Clone>(&self, event: &str, payload: P) {
        drop(self.emit(event, payload));
    }

    fn view(&self) -> View {
        self.actions().view(self)
    }
}

type Job = Box<dyn FnOnce() + Send>;

/// The lanes with actions waiting (see `prisme_actions::lane`), each emptied in turn by a
/// worker of its own, which ends as its lane does.
#[derive(Default)]
struct Lanes(Arc<Mutex<HashMap<Lane, VecDeque<Job>>>>);

/// A lane taken for an action to wait its turn on.
struct Queuing<'l> {
    waiting: MutexGuard<'l, HashMap<Lane, VecDeque<Job>>>,
    lanes: &'l Arc<Mutex<HashMap<Lane, VecDeque<Job>>>>,
    lane: Lane,
}

impl Lanes {
    /// `lane` taken for an action that waits its turn, if it has to (`waits`) or if the lane
    /// has actions waiting; none if it can be performed at once.
    fn take(&self, lane: Lane, waits: bool) -> Option<Queuing<'_>> {
        let waiting = lock(&self.0);
        (waits || waiting.contains_key(&lane)).then_some(Queuing { waiting, lanes: &self.0, lane })
    }

    fn busy(&self, lane: Lane) -> bool {
        lock(&self.0).contains_key(&lane)
    }
}

impl Queuing<'_> {
    /// `job` after those waiting on the lane; a lane with none gets a worker.
    fn push(mut self, job: Job) {
        if let Some(waiting) = self.waiting.get_mut(&self.lane) {
            waiting.push_back(job);
            return;
        }
        self.waiting.insert(self.lane, VecDeque::from([job]));
        drop(self.waiting);
        let (lanes, lane) = (self.lanes.clone(), self.lane);
        std::thread::Builder::new()
            .name("prisme-lane".into())
            .spawn(move || run_lane(&lanes, lane))
            .expect("cannot start a worker of the actions");
    }
}

/// Performs the actions waiting on `lane`, in turn, until there are none: the lane stays taken
/// while one runs.
fn run_lane(lanes: &Mutex<HashMap<Lane, VecDeque<Job>>>, lane: Lane) {
    loop {
        let job = {
            let mut waiting = lock(lanes);
            match waiting.get_mut(&lane).and_then(VecDeque::pop_front) {
                Some(job) => job,
                None => {
                    waiting.remove(&lane);
                    return;
                }
            }
        };
        if catch_unwind(AssertUnwindSafe(job)).is_err() {
            error!("an action of {lane:?} failed");
        }
    }
}

/// The end of an action performed in turn, which those sent after it in one move wait for.
#[derive(Default)]
struct Done {
    over: Mutex<bool>,
    ended: Condvar,
}

impl Done {
    fn is_over(&self) -> bool {
        *lock(&self.over)
    }

    fn wait(&self) {
        let over = lock(&self.over);
        drop(self.ended.wait_while(over, |over| !*over).unwrap_or_else(|e| e.into_inner()));
    }
}

/// Ends its action as it is dropped, whatever came of it.
struct Ending(Arc<Done>);

impl Drop for Ending {
    fn drop(&mut self) {
        *lock(&self.0.over) = true;
        self.0.ended.notify_all();
    }
}

/// The actions one move of a control drives, which keep their order whatever their lanes: one
/// sent after an action waiting its turn waits for it.
#[derive(Default)]
pub struct InTurn(Option<Arc<Done>>);

impl InTurn {
    fn waits(&self) -> bool {
        self.0.as_ref().is_some_and(|done| !done.is_over())
    }
}

/// Where the actions the interface sends are: those of its page loaded as `run`, the next to
/// perform numbered `next`.
#[derive(Default)]
struct Sent {
    run: u64,
    next: u64,
}

/// The place of an action among those the interface sends: its number among those of its page
/// loaded as `run`.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
pub struct Order {
    run: u64,
    number: u64,
}

/// The track selected in the library, as the interface tells it, and the last of the actions it
/// was told to perform (`ui-action`) it had applied then.
#[derive(Default)]
struct Selection {
    path: Option<PathBuf>,
    applied: u64,
}

pub struct Actions {
    /// What they keep between presses for themselves (see `prisme_actions::perform`): taken
    /// last, nothing else locked while it is held.
    memory: Mutex<Memory>,
    /// The controllers' faders picking their settings up, and their encoders' steps short of
    /// one.
    inputs: Mutex<Inputs>,
    /// What the loads load.
    selection: Mutex<Selection>,
    /// Told as the interface applies its actions.
    caught_up: Condvar,
    /// How many actions the interface was told to perform.
    told: AtomicU64,
    /// The actions the interface sent, performed in their order.
    sent: Mutex<Sent>,
    /// Told as one of them is performed.
    sent_on: Condvar,
    lanes: Lanes,
    /// The queue of the worker keeping in the library what the actions changed.
    kept: Sender<Job>,
    /// What the actions are timed by when no controller's clock tells it.
    started: Instant,
}

impl Actions {
    /// The actions, the worker keeping their changes in the library started.
    pub fn start() -> Self {
        let (kept, jobs) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("prisme-keep".into())
            .spawn(move || {
                for job in jobs {
                    if catch_unwind(AssertUnwindSafe(job)).is_err() {
                        error!("a change of the library not kept");
                    }
                }
            })
            .expect("cannot start the worker keeping the actions' changes");
        Self {
            memory: Mutex::default(),
            inputs: Mutex::default(),
            selection: Mutex::default(),
            caught_up: Condvar::new(),
            told: AtomicU64::new(0),
            sent: Mutex::default(),
            sent_on: Condvar::new(),
            lanes: Lanes::default(),
            kept,
            started: Instant::now(),
        }
    }

    /// Performs `target` driven by `input`, as `source` asks, after the actions `in_turn` sent
    /// before it: at once on this thread if its lane has none waiting and it opens no file, else
    /// in turn by its lane's worker (see `prisme_actions::lane`); `answer` hears how it went.
    /// Stamped with the time it is sent. Never called from the audio thread.
    pub(crate) fn perform<H: Host>(
        &self,
        host: &H,
        target: &Target,
        input: Input,
        source: Source,
        answer: Answer,
        in_turn: &mut InTurn,
    ) {
        let at_s = match &source {
            Source::Controller(moved) => moved.at_s,
            Source::Interface { .. } => self.now_s(),
        };
        let asked = Asked { source, at_s, told: self.told.load(Ordering::SeqCst) };
        let sent = self.sent_as(host.app_state(), target);
        let target = sent.as_ref().unwrap_or(target);
        let Some(queuing) = self.lanes.take(lane(target), in_turn.waits() || reads_files(target)) else {
            return answer.give(host, host.perform_here(target, input, asked));
        };
        let done = Arc::new(Done::default());
        let (after, ending) = (in_turn.0.replace(done.clone()), Ending(done));
        let (host, target, asked) = (host.clone(), target.clone(), asked.into_owned());
        queuing.push(Box::new(move || {
            let _ending = ending;
            if let Some(after) = after {
                after.wait();
            }
            answer.give(&host, host.perform_here(&target, input, asked));
        }));
    }

    /// What `target` is sent as: a load on the first deck without a track, on that deck; a smart
    /// pad on an empty slot, a load (unless its slot has a load waiting, which may fill it).
    fn sent_as(&self, state: &AppState, target: &Target) -> Option<Target> {
        match *target {
            Target::Library(LibraryAction::LoadFree) => {
                Some(Target::Library(LibraryAction::Load(self.free_deck(state))))
            }
            Target::Deck(deck, DeckAction::Sample(slot, SampleAction::Smart))
                if !self.lanes.busy(Lane::Sample(deck, slot))
                    && lock(&state.samplers)[deck][slot].is_none() =>
            {
                Some(Target::Deck(deck, DeckAction::Sample(slot, SampleAction::Load)))
            }
            _ => None,
        }
    }

    /// The first deck in use without a track nor a load waiting, else deck A.
    fn free_deck(&self, state: &AppState) -> usize {
        let decks = state.preferences.read(|p| p.dj.decks());
        let tracks = lock(&state.tracks);
        (0..decks).find(|&deck| tracks[deck].is_none() && !self.lanes.busy(Lane::Track(deck))).unwrap_or(0)
    }

    /// Sends `send`, an action of the interface, after those it numbered before it: their calls
    /// may reach Prisme in another order. One that does not come is waited for [`ORDER_WAIT`] at
    /// most.
    fn in_order(&self, Order { run, number: order }: Order, send: impl FnOnce()) {
        {
            let mut sent = lock(&self.sent);
            if sent.run != run {
                *sent = Sent { run, next: order };
            }
            let turn = |s: &mut Sent| s.run == run && s.next < order;
            drop(self.sent_on.wait_timeout_while(sent, ORDER_WAIT, turn));
        }
        send();
        let mut sent = lock(&self.sent);
        if sent.run == run {
            sent.next = sent.next.max(order + 1);
        }
        self.sent_on.notify_all();
    }

    /// The number of an action the interface is told to perform (see [`Actions::select`]).
    pub(crate) fn tell_interface(&self) -> u64 {
        self.told.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// `path` selected in the library, as the interface tells it, having applied the actions
    /// it was told to perform up to the one numbered `applied`, if it says.
    pub fn select(&self, path: Option<PathBuf>, applied: Option<u64>) {
        let mut selection = lock(&self.selection);
        selection.path = path;
        selection.applied = selection.applied.max(applied.unwrap_or(0));
        self.caught_up.notify_all();
    }

    /// The track selected once the interface applied the actions it was told to perform before
    /// an action asked after `told` of them: a browse sent before a load moves what it loads.
    /// Waited for [`SELECTION_WAIT`] at most, then the selection as it is (a window asleep), not
    /// waited for again for those actions. Off the threads of the controllers.
    fn selected(&self, told: u64) -> Option<PathBuf> {
        let selection = lock(&self.selection);
        let (mut selection, waited) = self
            .caught_up
            .wait_timeout_while(selection, SELECTION_WAIT, |s| s.applied < told)
            .unwrap_or_else(|e| e.into_inner());
        if waited.timed_out() {
            selection.applied = told;
        }
        selection.path.clone()
    }

    /// `job` done in turn on `lane`, after the actions waiting there (the Auto DJ's load of a
    /// deck, say).
    pub(crate) fn in_turn_on(&self, lane: Lane, job: impl FnOnce() + Send + 'static) {
        if let Some(queuing) = self.lanes.take(lane, true) {
            queuing.push(Box::new(job));
        }
    }

    /// Keeps in the library what `write` writes, in turn after what was kept before: the
    /// change is in memory and in the engine already.
    pub(crate) fn keep(&self, write: impl FnOnce() + Send + 'static) {
        // the worker lives as long as the application
        let _ = self.kept.send(Box::new(write));
    }

    /// Seconds since Prisme started.
    fn now_s(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// What a controller `device`'s `input` gives `target`, an action the interface performs: a
    /// key's value, an encoder's whole steps (those short of one kept for its next move); none
    /// for an encoder on a key.
    pub fn interface_value(&self, device: &str, target: &Target, input: Input) -> Option<f32> {
        match lock(&self.inputs).resolve(device, target, input, None)? {
            Given::Value(value) => Some(value),
            Given::Steps(steps) => Some(steps as f32),
        }
    }

    /// The application as the actions see it now.
    pub fn view(&self, app: &AppHandle) -> View {
        let auto_dj = app.try_state::<AutoDj>().is_some_and(|a| a.plays());
        view_of(&app.state(), auto_dj, &self.memory, self.now_s())
    }

    /// Waits for the lanes to have no action waiting, then for what they changed to be kept.
    #[cfg(test)]
    pub fn settle(&self) {
        while !lock(&self.lanes.0).is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let (kept, over) = mpsc::sync_channel(1);
        self.keep(move || {
            let _ = kept.send(());
        });
        let _ = over.recv();
    }
}

/// An action being performed on the application's state, as it was asked.
struct Performing<'a> {
    state: &'a AppState,
    actions: &'a Actions,
    /// What the Auto DJ and the library's edits are reached by: none in a test.
    app: Option<&'a AppHandle>,
    /// Where the cues are kept, once open.
    library: Option<&'a Arc<LibraryDb>>,
    asked: Asked<'a>,
}

impl Performing<'_> {
    /// The Auto DJ, if it runs.
    fn auto_dj(&self) -> Option<tauri::State<'_, AutoDj>> {
        self.app.and_then(|app| app.try_state::<AutoDj>())
    }

    /// What the pads hold, which the interface shows.
    fn perform_state(&self) -> MirroredGuard<'_, PerformState> {
        lock(&self.state.perform)
    }

    /// What the actions keep for themselves: let go before any other lock is taken.
    fn memory(&self) -> MutexGuard<'_, Memory> {
        lock(&self.actions.memory)
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

    /// Seconds on the clock of the one asking as the action was sent: a controller's own, else
    /// Prisme's.
    fn at_s(&self) -> f64 {
        self.asked.at_s
    }

    /// Who asks.
    fn source(&self) -> &Source<'_> {
        &self.asked.source
    }

    /// The track selected in the library, once the interface applied the actions it was told to
    /// perform before this one (see [`Actions::selected`]).
    fn selected(&self) -> Option<PathBuf> {
        self.actions.selected(self.asked.told)
    }

    /// Keeps in the library, once it is open, what `write` writes there (see [`Actions::keep`]).
    fn keep(&self, what: impl FnOnce(&LibraryDb) -> Result<(), prisme_library::DbError> + Send + 'static) {
        let Some(library) = self.library.cloned() else { return };
        self.actions.keep(move || {
            if let Err(e) = what(&library) {
                error!("a change of the library not kept: {e}");
            }
        });
    }

    /// What `input` gives `target`: the interface's faders set what they show, a controller's
    /// pick their setting up first (see `Inputs`); nothing while it gives nothing. Only where
    /// its fader stands is read.
    fn given(&self, target: &Target, input: Input) -> Option<Given> {
        match (&self.asked.source, input) {
            (Source::Interface { .. }, Input::Value(value)) => Some(Given::Value(value)),
            // the interface's encoders under a name no controller has
            (Source::Interface { .. }, steps) => {
                let at = position(target, self);
                lock(&self.actions.inputs).resolve("", target, steps, at)
            }
            (Source::Controller(moved), input) => {
                let at = position(target, self);
                lock(&self.actions.inputs).resolve(&moved.device, target, input, at)
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

/// Performs the action `target` driven to `value` (an encoder's `steps`, if it says so), as the
/// interface asks: `confirmed`, the DJ said yes to what it replaces. The
/// interface numbers its actions (`order`): each is sent in turn after those sent before it, a
/// key let go after it was pressed.
#[tauri::command]
pub async fn perform_action(
    app: AppHandle,
    target: String,
    value: f32,
    steps: bool,
    confirmed: bool,
    order: Order,
) -> AppResult<()> {
    let target: Target = target.parse().map_err(|e: Unknown| AppError::Other(e.to_string()))?;
    let input = Input::of(value, steps, None);
    let source = Source::Interface { confirmed };
    crate::commands::blocking(move || {
        let actions = app.state::<Actions>();
        let (answer, outcome) = mpsc::sync_channel(1);
        actions.in_order(order, || {
            let in_turn = &mut InTurn::default();
            actions.perform(&app, &target, input, source, Answer::To(answer), in_turn);
        });
        outcome.recv().unwrap_or_else(|e| Err(AppError::Other(e.to_string())))
    })
    .await
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

    /// How long a track of the tests lasts (s).
    pub const TRACK_S: f64 = 10.0;

    /// A quiet track of [`TRACK_S`] on the disk, written for the test `name` to be loaded as a
    /// DJ would, and removed with it.
    pub struct Track(pub PathBuf);

    impl Drop for Track {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    pub fn track(name: &str) -> Track {
        let path = std::env::temp_dir().join(format!("prisme-bench-{name}-{}.wav", std::process::id()));
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for _ in 0..(TRACK_S * 48_000.0) as usize * 2 {
            writer.write_sample(0i16).unwrap();
        }
        writer.finalize().unwrap();
        Track(path)
    }

    pub struct Bench {
        pub state: AppState,
        pub actions: Actions,
        pub library: Arc<LibraryDb>,
        engine: Mutex<Engine>,
        /// What the interface was told, in turn.
        told: Mutex<Vec<(String, serde_json::Value)>>,
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
            let library = Arc::new(LibraryDb::open_in_memory().unwrap());
            Self {
                state,
                actions: Actions::start(),
                library,
                engine: Mutex::new(engine),
                told: Mutex::default(),
            }
        }

        /// `path` selected in the library, as the interface tells it, every action it was told
        /// applied.
        pub fn select(&self, path: &std::path::Path) {
            let told = self.actions.told.load(Ordering::SeqCst);
            self.actions.select(Some(path.to_owned()), Some(told));
        }

        /// What the interface was told since last asked: each event and what it carried.
        pub fn told(&self) -> Vec<(String, serde_json::Value)> {
            std::mem::take(&mut *lock(&self.told))
        }

        /// The application as the actions see it now.
        pub fn view(&self) -> View {
            view_of(&self.state, false, &self.actions.memory, 0.0)
        }

        /// `id` driven to `value` by the controller `device`.
        pub fn moved(&self, device: &str, id: &str, input: Input) -> AppResult<()> {
            let source =
                Source::Controller(Moved { device: device.into(), at_s: 0.0, jog_steps_per_turn: 720.0 });
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

        /// An action asked for by `source`, now.
        pub(in crate::actions) fn performing<'a>(&'a self, source: Source<'a>) -> Performing<'a> {
            let at_s = match &source {
                Source::Controller(moved) => moved.at_s,
                Source::Interface { .. } => self.actions.now_s(),
            };
            self.asked(Asked { source, at_s, told: self.actions.told.load(Ordering::SeqCst) })
        }

        fn asked<'a>(&'a self, asked: Asked<'a>) -> Performing<'a> {
            Performing {
                state: &self.state,
                actions: &self.actions,
                app: None,
                library: Some(&self.library),
                asked,
            }
        }

        /// Where the fader of `id` stands.
        pub fn position(&self, id: &str) -> Option<f32> {
            position(&id.parse().unwrap(), &self.view())
        }

        /// The commands the engine was sent since last asked, not applied.
        pub fn sent(&self) -> Vec<Command> {
            lock(&self.engine).take_commands()
        }

        /// The engine run for `seconds`, a period of 10 ms at least: what it was sent applied,
        /// its state told.
        pub fn run(&self, seconds: f64) {
            let frames = 480;
            let (input, mut output) = (vec![0.0; frames * 2], vec![0.0; frames * 2]);
            let mut engine = lock(&self.engine);
            for _ in 0..(seconds / 0.01).round().max(1.0) as usize {
                engine.process(&input, 2, &mut output, 2);
            }
        }

        /// A silent track of `seconds` on `deck`, with a beat grid at `bpm` from its start if
        /// it has one, stopped at its start.
        pub fn load(&self, deck: usize, seconds: f64, bpm: Option<f64>) {
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
            self.state.put_track(&self.state.ask_load(deck), Some(track), commands).unwrap();
            self.run(0.01);
        }

        /// The track on `deck` put at `seconds` on the screen, the engine run.
        pub fn at(&self, deck: usize, seconds: f64) {
            let duration_s = self.status(deck).duration_s;
            let input = Input::Value((seconds / duration_s) as f32);
            self.perform(
                &format!("deck{}.position", deck + 1),
                input,
                Source::Interface { confirmed: false },
            )
            .unwrap();
            self.run(0.01);
        }

        /// What the engine shows of `deck`.
        pub fn status(&self, deck: usize) -> prisme_engine::DeckStatus {
            self.state.deck_status(deck).unwrap()
        }
    }

    /// The bench reached as the application is: what the controllers drive performed in turn
    /// on the lanes, the interface's events kept.
    impl Host for Arc<Bench> {
        fn actions(&self) -> &Actions {
            &self.actions
        }

        fn app_state(&self) -> &AppState {
            &self.state
        }

        fn perform_here(&self, target: &Target, input: Input, asked: Asked) -> AppResult<()> {
            if target.owner() == Owner::Ui {
                return Err(AppError::NotPerformed(target.to_string()));
            }
            self.asked(asked).perform(target, input)
        }

        fn tell<P: Serialize + Clone>(&self, event: &str, payload: P) {
            lock(&self.told).push((event.to_owned(), serde_json::to_value(payload).unwrap()));
        }

        fn view(&self) -> View {
            Bench::view(self)
        }
    }
}

#[cfg(test)]
mod lanes {
    //! The lanes run by their workers, as the application's are.

    use super::bench::{track, Bench};
    use super::*;
    use prisme_library::CueSlot;
    use std::path::Path;
    use std::time::Duration;

    /// `id` driven to `value` by a controller, after what `in_turn` sent.
    fn sent(bench: &Arc<Bench>, id: &str, value: f32, in_turn: &mut InTurn) {
        let source = Source::Controller(Moved { device: "DDJ".into(), at_s: 0.0, jog_steps_per_turn: 720.0 });
        let target: Target = id.parse().unwrap();
        bench.actions.perform(bench, &target, Input::Value(value), source, Answer::Told, in_turn);
    }

    fn pressed(bench: &Arc<Bench>, id: &str) {
        for value in [1.0, 0.0] {
            sent(bench, id, value, &mut InTurn::default());
        }
    }

    /// A job holding `lane` until the sender it gives is dropped or sent to.
    fn held(bench: &Bench, lane: Lane) -> SyncSender<()> {
        let (release, wait) = mpsc::sync_channel(1);
        bench.actions.in_turn_on(lane, move || {
            let _ = wait.recv_timeout(Duration::from_secs(5));
        });
        release
    }

    #[test]
    fn the_actions_of_one_move_keep_their_order_whatever_their_lanes() {
        let bench = Arc::new(Bench::new());
        bench.state.update_preferences(|p| p.dj.cue_on_load = true);
        let file = track("in-turn");
        bench.select(&file.0);
        // a load heard alone in the headphones, then the deck beside it heard too
        let in_turn = &mut InTurn::default();
        sent(&bench, "library.load1", 1.0, in_turn);
        sent(&bench, "deck2.headphones.on", 1.0, in_turn);
        bench.actions.settle();
        assert!(lock(&bench.state.tracks)[0].is_some(), "loaded");
        assert_eq!(lock(&bench.state.decks).map(|d| d.cue)[..2], [true, true]);
    }

    #[test]
    fn a_hot_cue_is_set_at_once_while_another_deck_loads() {
        let bench = Arc::new(Bench::new());
        bench.load(0, 60.0, None);
        let file = track("loading");
        bench.select(&file.0);
        let loading = held(&bench, Lane::Track(1));
        sent(&bench, "library.load2", 1.0, &mut InTurn::default());
        pressed(&bench, "deck1.hotcue1");
        assert!(bench.state.with_track(0, |t| t.cue(CueSlot::Hot(0)).is_some()).unwrap(), "not delayed");
        assert!(lock(&bench.state.tracks)[1].is_none(), "deck B still loads");
        loading.send(()).unwrap();
        bench.actions.settle();
        assert!(lock(&bench.state.tracks)[1].is_some());
    }

    #[test]
    fn an_action_on_a_deck_that_loads_waits_for_its_track() {
        let bench = Arc::new(Bench::new());
        let file = track("then-play");
        bench.select(&file.0);
        let loading = held(&bench, Lane::Track(0));
        sent(&bench, "library.load1", 1.0, &mut InTurn::default());
        pressed(&bench, "deck1.play");
        bench.run(0.01);
        assert!(!bench.status(0).playing, "waits");
        loading.send(()).unwrap();
        bench.actions.settle();
        bench.run(0.05);
        assert!(bench.status(0).playing, "the track loaded plays");
    }

    #[test]
    fn cue_then_play_play_while_the_library_is_busy() {
        let bench = Arc::new(Bench::new());
        bench.load(0, 60.0, None);
        bench.at(0, 5.0);
        let (release, wait) = mpsc::sync_channel::<()>(1);
        bench.actions.keep(move || {
            let _ = wait.recv_timeout(Duration::from_secs(5));
        });
        sent(&bench, "deck1.cue", 1.0, &mut InTurn::default());
        pressed(&bench, "deck1.play");
        sent(&bench, "deck1.cue", 0.0, &mut InTurn::default());
        bench.run(0.05);
        assert!(bench.status(0).playing);
        let cue_s = bench.state.with_track(0, |t| t.cue(CueSlot::Main).map(|c| c.position_s)).unwrap();
        assert!(cue_s.is_some_and(|s| (s - 5.0).abs() < 1e-3), "set at once: {cue_s:?}");
        assert!(bench.library.cues(Path::new("deck0.flac")).unwrap().is_empty(), "not kept yet");
        release.send(()).unwrap();
        bench.actions.settle();
        assert_eq!(bench.library.cues(Path::new("deck0.flac")).unwrap().len(), 1, "kept");
    }

    #[test]
    fn a_tap_is_timed_as_it_is_sent_not_as_it_is_performed() {
        let bench = Arc::new(Bench::new());
        bench.load(0, 60.0, Some(100.0));
        let busy = held(&bench, Lane::Track(0));
        let tap = |at_s: f64| {
            let source = Source::Controller(Moved { device: "DDJ".into(), at_s, jog_steps_per_turn: 720.0 });
            let tap: Target = "deck1.tap".parse().unwrap();
            bench.actions.perform(
                &bench,
                &tap,
                Input::Value(1.0),
                source,
                Answer::Told,
                &mut InTurn::default(),
            );
        };
        for at_s in [0.0, 0.5, 1.0, 1.5] {
            tap(at_s);
        }
        busy.send(()).unwrap();
        bench.actions.settle();
        let mut taps = lock(&bench.actions.memory).decks[0].taps.clone();
        assert_eq!(taps.tap(2.0).map(|bpm| bpm.round()), Some(120.0), "taps half a second apart");
    }
}

#[cfg(test)]
mod selection {
    //! The loads of a controller, browsing on the screen.

    use super::bench::{track, Bench};
    use super::*;
    use std::time::Duration;

    fn load(bench: &Arc<Bench>, id: &str) {
        let source = Source::Controller(Moved { device: "CDJ".into(), at_s: 0.0, jog_steps_per_turn: 720.0 });
        let target: Target = id.parse().unwrap();
        bench.actions.perform(
            bench,
            &target,
            Input::Value(1.0),
            source,
            Answer::Told,
            &mut InTurn::default(),
        );
        bench.actions.settle();
    }

    fn loaded(bench: &Bench, deck: usize) -> Option<PathBuf> {
        lock(&bench.state.tracks)[deck].as_ref().map(|t| t.path.clone())
    }

    #[test]
    fn a_load_takes_the_track_the_browsing_sent_before_it_selects() {
        let bench = Arc::new(Bench::new());
        let (first, next) = (track("first"), track("next"));
        bench.select(&first.0);
        // the browse goes to the interface, which moves a moment later
        let browse = bench.actions.tell_interface();
        let (interface, path) = (bench.clone(), next.0.clone());
        let moving = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            interface.actions.select(Some(path), Some(browse));
        });
        load(&bench, "library.load1");
        moving.join().unwrap();
        assert_eq!(loaded(&bench, 0), Some(next.0.clone()));
    }

    #[test]
    fn a_window_asleep_lets_the_load_take_the_selection_it_has_once() {
        let bench = Arc::new(Bench::new());
        let first = track("asleep");
        bench.select(&first.0);
        bench.actions.tell_interface();
        let waiting = Instant::now();
        load(&bench, "library.load1");
        assert!(waiting.elapsed() >= SELECTION_WAIT);
        assert_eq!(loaded(&bench, 0), Some(first.0.clone()));
        let again = Instant::now();
        load(&bench, "library.load2");
        assert!(again.elapsed() < SELECTION_WAIT, "not waited for again");
        assert_eq!(loaded(&bench, 1), Some(first.0.clone()));
    }
}

#[cfg(test)]
mod interface_order {
    use super::*;
    use std::time::Duration;

    /// Sends the interface's actions `orders` of `run`, each from a thread of its own a moment
    /// after the one before: the order they are performed in.
    fn sent(actions: &Arc<Actions>, run: u64, orders: &[u64]) -> Vec<u64> {
        let performed = Arc::new(Mutex::new(Vec::new()));
        let threads: Vec<_> = orders
            .iter()
            .map(|&order| {
                let (actions, performed) = (actions.clone(), performed.clone());
                std::thread::sleep(Duration::from_millis(5));
                let numbered = Order { run, number: order };
                std::thread::spawn(move || actions.in_order(numbered, || lock(&performed).push(order)))
            })
            .collect();
        threads.into_iter().for_each(|t| t.join().unwrap());
        Arc::try_unwrap(performed).unwrap().into_inner().unwrap()
    }

    #[test]
    fn the_actions_of_the_interface_are_performed_in_the_order_it_sent_them() {
        let actions = Arc::new(Actions::start());
        assert_eq!(sent(&actions, 7, &[1]), [1]);
        assert_eq!(sent(&actions, 7, &[3, 2]), [2, 3], "a release overtaking its press waits for it");
        let lost = Instant::now();
        assert_eq!(sent(&actions, 7, &[5]), [5], "the one before lost");
        assert!(lost.elapsed() >= ORDER_WAIT);
        assert_eq!(sent(&actions, 8, &[1, 2]), [1, 2], "the page loaded again");
    }
}
