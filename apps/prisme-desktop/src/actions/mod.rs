//! The actions of the application, with one way in for all who ask for them
//! ([`Actions::perform`]): the controllers from their threads, and the interface
//! ([`Actions::perform_asked`]). Each action keeps its order among those of its lane
//! (`prisme_actions::lane`): it is done at once on the thread asking for it while its lane has
//! none waiting, else in turn by the lane's worker, as one opening a file always is; the actions
//! one move drives keep their order whatever their lanes. What an action changes of the library
//! (a cue, a beat grid) is in memory and in the engine at once, and kept in the library in turn
//! by a worker of its own ([`Actions::keep`]). What each action does is in `perform`; the zones'
//! modules hold what the actions and the interface's commands both do, done once. What the
//! actions keep between presses is here, and the view of the application the controllers'
//! lights and values are read from ([`Actions::view`]). The actions the interface performs (its
//! windows, the focus of its search), a controller tells it (`ui-action`).

pub mod browse;
mod cues;
pub mod deck;
pub mod edits;
pub mod fx;
pub mod general;
mod keys;
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
use crate::state::{lock, AppState, MirroredGuard, PadsShown};
use keys::{Keys, Press};
use prisme_actions::input::{Given, Input, Inputs};
use prisme_actions::lane::{follows, lane, reads_files, takes_selection, Lane};
use prisme_actions::perform::{Memory, PerformState};
use prisme_actions::position::position;
use prisme_actions::view::View;
use prisme_control::{DeckAction, Kind, LibraryAction, Owner, SampleAction, Target, Unknown};
use prisme_engine::EngineStatus;
use prisme_library::LibraryDb;
use serde::Serialize;
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender, SyncSender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tracing::{error, warn};

pub use view::{view_of, view_with};

/// A failure of an action a controller asked for, for the interface to word (an `AppError`).
pub const FAILED_EVENT: &str = "action-failed";
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

impl Source<'_> {
    /// The name of the controller asking, empty for the interface.
    fn device(&self) -> &str {
        match self {
            Source::Controller(moved) => &moved.device,
            Source::Interface { .. } => "",
        }
    }
}

/// An action asked for, as it was sent, whenever it is performed: by whom, when on the clock of
/// the one asking (a controller's own, else Prisme's), and the track of the library it takes (a
/// load, a preview): the one the interface named, else the one selected once the library's
/// actions sent before it are over.
#[derive(Debug, Clone)]
pub struct Asked<'d> {
    pub source: Source<'d>,
    pub at_s: f64,
    pub track: Option<PathBuf>,
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
            (Answer::Told, Err(e)) if host.actions().tells(&e) => host.tell(FAILED_EVENT, &e),
            (Answer::Told, _) => {}
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
        if target.owner() != Owner::Rust {
            return Err(AppError::NotPerformed(target.to_string()));
        }
        let library = self.try_state::<Arc<LibraryService>>();
        let library = library.as_ref().map(|l| &l.db);
        let (state, actions) = (self.app_state(), self.actions());
        Performing { state, actions, app: Some(self), library, asked }.perform(target, input)
    }

    fn tell<P: Serialize + Clone>(&self, event: &str, payload: P) {
        if let Err(e) = self.emit(event, payload) {
            warn!("the interface not told {event}: {e}");
        }
    }

    fn view(&self) -> View {
        self.actions().view(self)
    }
}

type Job = Box<dyn FnOnce() + Send>;

/// The actions waiting their turn (see `prisme_actions::lane`): those queued on each busy lane,
/// which a worker of its own performs in turn and ends with; and the actions of a move set
/// aside until those sent before them in the move are over.
#[derive(Default)]
struct Lanes(Arc<Mutex<Waiting>>);

#[derive(Default)]
struct Waiting {
    queued: HashMap<Lane, VecDeque<Job>>,
    /// The last action each controller set aside for a lane: its later actions on that lane
    /// follow it.
    aside: Vec<Aside>,
}

struct Aside {
    lane: Lane,
    device: String,
    turn: Arc<Turn>,
}

impl Waiting {
    /// The action `device` last set aside for `lane`, while it is not over.
    fn aside_on(&mut self, lane: Lane, device: &str) -> Option<Arc<Turn>> {
        if self.aside.is_empty() {
            return None;
        }
        self.aside.retain(|aside| !aside.turn.is_over());
        let aside = self.aside.iter().find(|aside| aside.lane == lane && aside.device == device)?;
        Some(aside.turn.clone())
    }

    /// `turn`, an action of `device` set aside for `lane`, the last there.
    fn set_aside(&mut self, lane: Lane, device: &str, turn: Arc<Turn>) {
        match self.aside.iter_mut().find(|aside| aside.lane == lane && aside.device == device) {
            Some(aside) => aside.turn = turn,
            None => self.aside.push(Aside { lane, device: device.to_owned(), turn }),
        }
    }
}

impl Lanes {
    /// `job` after those queued on `lane` (`waiting`, this lock held); a lane with none gets a
    /// worker, without which its jobs are dropped.
    fn push(&self, mut waiting: MutexGuard<'_, Waiting>, lane: Lane, job: Job) {
        if let Some(queued) = waiting.queued.get_mut(&lane) {
            queued.push_back(job);
            return;
        }
        waiting.queued.insert(lane, VecDeque::from([job]));
        drop(waiting);
        let lanes = self.0.clone();
        let started =
            std::thread::Builder::new().name("prisme-lane".into()).spawn(move || run_lane(&lanes, lane));
        if let Err(e) = started {
            error!("no worker for the actions of {lane:?}: {e}");
            let dropped = lock(&self.0).queued.remove(&lane);
            // dropped once the lock is let go: their ends tell what follows them to go on
            drop(dropped);
        }
    }

    fn busy(&self, lane: Lane) -> bool {
        lock(&self.0).queued.contains_key(&lane)
    }
}

/// Performs the actions queued on `lane`, in turn, until there are none: the lane stays busy
/// while one runs.
fn run_lane(lanes: &Mutex<Waiting>, lane: Lane) {
    loop {
        let job = {
            let mut waiting = lock(lanes);
            match waiting.queued.get_mut(&lane).and_then(VecDeque::pop_front) {
                Some(job) => job,
                None => {
                    waiting.queued.remove(&lane);
                    return;
                }
            }
        };
        if catch_unwind(AssertUnwindSafe(job)).is_err() {
            error!("an action of {lane:?} failed");
        }
    }
}

/// An action waiting its turn, and the actions sent once it is over.
struct Turn(Mutex<Then>);

enum Then {
    Waiting(Vec<Arc<Follower>>),
    Over,
}

impl Turn {
    fn new() -> Arc<Self> {
        Arc::new(Turn(Mutex::new(Then::Waiting(Vec::new()))))
    }

    fn is_over(&self) -> bool {
        matches!(*lock(&self.0), Then::Over)
    }

    /// `follower` told once this is over; false if it is already.
    fn then(&self, follower: &Arc<Follower>) -> bool {
        match &mut *lock(&self.0) {
            Then::Waiting(followers) => {
                followers.push(follower.clone());
                true
            }
            Then::Over => false,
        }
    }

    /// Over: what followed it is sent, its lock let go first (sending takes the lanes').
    fn end(&self) {
        let then = std::mem::replace(&mut *lock(&self.0), Then::Over);
        if let Then::Waiting(followers) = then {
            followers.iter().for_each(|follower| follower.one_over());
        }
    }
}

/// Ends its action as it is dropped, whatever came of it.
struct Ending(Arc<Turn>);

impl Drop for Ending {
    fn drop(&mut self) {
        self.0.end();
    }
}

/// The end of an action's turn, once it waits: made then, the last of its move (`in_turn`)
/// from then, or made already as it was set aside.
enum Ends<'t> {
    Move(&'t mut InTurn),
    Made(Ending),
}

impl Ends<'_> {
    fn made(self) -> Ending {
        match self {
            Ends::Move(in_turn) => {
                let turn = Turn::new();
                in_turn.0 = Some(turn.clone());
                Ending(turn)
            }
            Ends::Made(ending) => ending,
        }
    }
}

/// Performs `target` driven by `input` as `asked`, here: one that panics fails as any other,
/// the thread asking (a controller's, the interface's turn) going on.
fn perform_here<H: Host>(host: &H, target: &Target, input: Input, asked: Asked) -> AppResult<()> {
    catch_unwind(AssertUnwindSafe(|| host.perform_here(target, input, asked))).unwrap_or_else(|_| {
        error!("{target} failed");
        Err(AppError::Other(format!("{target} failed")))
    })
}

/// An action sent once the actions it follows are all over.
struct Follower {
    before: AtomicUsize,
    send: Mutex<Option<Job>>,
}

impl Follower {
    /// `send` run once every one of `turns` is over: at once, on this thread, if they are.
    fn after(turns: impl IntoIterator<Item = Arc<Turn>>, send: Job) {
        let follower = Arc::new(Follower { before: AtomicUsize::new(1), send: Mutex::new(Some(send)) });
        for turn in turns {
            follower.before.fetch_add(1, Ordering::SeqCst);
            if !turn.then(&follower) {
                follower.before.fetch_sub(1, Ordering::SeqCst);
            }
        }
        follower.one_over();
    }

    fn one_over(&self) {
        if self.before.fetch_sub(1, Ordering::SeqCst) != 1 {
            return;
        }
        if let Some(send) = lock(&self.send).take() {
            send_ready(send);
        }
    }
}

thread_local! {
    /// The followers this thread sends, while it sends one: those an action it sent let go
    /// wait for it to be over, so that a chain of thousands set aside never deepens the stack.
    static READY: RefCell<Option<VecDeque<Job>>> = const { RefCell::new(None) };
}

/// Runs `send`, an action whose turn came, here; or after the one this thread is sending, if
/// it is (an action it sent ended). Run as an action it follows ends, maybe unwinding from its
/// failure.
fn send_ready(send: Job) {
    let sending = READY.with_borrow_mut(|ready| match ready {
        Some(ready) => {
            ready.push_back(send);
            None
        }
        None => {
            *ready = Some(VecDeque::new());
            Some(send)
        }
    });
    let mut next = sending;
    while let Some(send) = next {
        if catch_unwind(AssertUnwindSafe(send)).is_err() {
            error!("an action sent in turn failed");
        }
        next = READY.with_borrow_mut(|ready| {
            let next = ready.as_mut().and_then(VecDeque::pop_front);
            if next.is_none() {
                *ready = None;
            }
            next
        });
    }
}

/// The actions one move of a control drives, which keep their order whatever their lanes: one
/// sent after an action waiting its turn is set aside, its lane free for others, and sent once
/// that one is over.
#[derive(Default)]
pub struct InTurn(Option<Arc<Turn>>);

impl InTurn {
    /// The last action of the move, while it waits its turn.
    fn waiting(&self) -> Option<Arc<Turn>> {
        self.0.as_ref().filter(|turn| !turn.is_over()).cloned()
    }
}

/// Where the actions the interface sends are: those of its page loaded as `run`, the one whose
/// turn it is numbered `next`, `started` once it came, and the numbers come and waiting.
#[derive(Default)]
struct Sent {
    run: u64,
    next: u64,
    started: bool,
    waiting: Vec<u64>,
}

/// The place of an action among those the interface sends: its number among those of its page
/// loaded as `run`.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
pub struct Order {
    run: u64,
    number: u64,
}

pub struct Actions {
    /// What they keep between presses for themselves (see `prisme_actions::perform`): taken
    /// last, nothing else locked while it is held.
    memory: Mutex<Memory>,
    /// The controllers' faders picking their settings up, and their encoders' steps short of
    /// one.
    inputs: Mutex<Inputs>,
    /// The actions the interface sent, performed in their order.
    sent: Mutex<Sent>,
    /// Told as one of them is performed.
    sent_on: Condvar,
    lanes: Lanes,
    /// The queue of the worker keeping in the library what the actions changed.
    kept: Sender<Job>,
    /// What the actions are timed by when no controller's clock tells it.
    started: Instant,
    /// The interface's keys held that act while they are.
    keys: Keys,
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
            sent: Mutex::default(),
            sent_on: Condvar::new(),
            lanes: Lanes::default(),
            kept,
            started: Instant::now(),
            keys: Keys::default(),
        }
    }

    /// Performs `target` driven by `input`, as `source` asks, after the actions `in_turn` sent
    /// before it: at once on this thread if its lane has none waiting and it opens no file, else
    /// in turn by its lane's worker (see `prisme_actions::lane`); `answer` hears how it went.
    /// One following an action still waiting its turn, of its move or the last its controller
    /// set aside for its lane, is set aside until those are over, its lane left to the others;
    /// one reading what another lane's actions leave (a load, the selection) waits for those
    /// sent there before it. Stamped with the time it is sent. Never called from the audio
    /// thread.
    pub(crate) fn perform<H: Host>(
        &self,
        host: &H,
        target: &Target,
        input: Input,
        source: Source,
        answer: Answer,
        in_turn: &mut InTurn,
    ) {
        self.send_asked(host, target, input, self.asked(source, None), answer, in_turn);
    }

    /// Loads the track at `path` (not the selection) on `deck`, or on the first deck without a
    /// track (deck A if none), as the interface asks: `confirmed`, the DJ said yes to what it
    /// replaces.
    pub(crate) fn load_track<H: Host>(
        &self,
        host: &H,
        path: PathBuf,
        deck: Option<usize>,
        confirmed: bool,
        answer: Answer,
    ) {
        let target = Target::Library(deck.map_or(LibraryAction::LoadFree, LibraryAction::Load));
        let asked = self.asked(Source::Interface { confirmed }, Some(path));
        self.send_asked(host, &target, Input::Value(1.0), asked, answer, &mut InTurn::default());
    }

    /// Performs `target` driven by `input`, `asked` as it was sent (see [`Actions::perform`]).
    /// One taking the selection takes it as it is when it is sent, or once what it follows is
    /// over if it does.
    fn send_asked<H: Host>(
        &self,
        host: &H,
        target: &Target,
        input: Input,
        asked: Asked,
        answer: Answer,
        in_turn: &mut InTurn,
    ) {
        let sent = self.sent_as(host.app_state(), target);
        let target = sent.as_ref().unwrap_or(target);
        let lane = lane(target);
        let takes = takes_selection(target) && asked.track.is_none();
        let mut waiting = lock(&self.lanes.0);
        let aside = waiting.aside_on(lane, asked.source.device());
        let after = in_turn.waiting();
        let before = follows(target, !takes).filter(|before| waiting.queued.contains_key(before));
        if after.is_none() && aside.is_none() && before.is_none() {
            drop(waiting);
            let track = if takes { browse::selected(host.app_state()) } else { asked.track };
            let asked = Asked { track, ..asked };
            return self.perform_or_queue(host, target, input, asked, answer, Ends::Move(in_turn));
        }
        let ending = Ends::Move(in_turn).made();
        waiting.set_aside(lane, asked.source.device(), ending.0.clone());
        let (host, target, mut asked) = (host.clone(), target.clone(), asked.into_owned());
        let send = move || {
            if takes {
                asked.track = browse::selected(host.app_state());
            }
            host.actions().perform_or_queue(&host, &target, input, asked, answer, Ends::Made(ending));
        };
        let turns: Vec<_> = after.into_iter().chain(aside).collect();
        match before {
            // in turn behind the actions whose outcome it reads
            Some(before) => {
                let follow = move || Follower::after(turns, Box::new(send));
                self.lanes.push(waiting, before, Box::new(follow));
            }
            None => {
                drop(waiting);
                Follower::after(turns, Box::new(send));
            }
        }
    }

    /// An action `source` asks for now, taking `track` of the library.
    fn asked<'d>(&self, source: Source<'d>, track: Option<PathBuf>) -> Asked<'d> {
        let at_s = match &source {
            Source::Controller(moved) => moved.at_s,
            Source::Interface { .. } => self.now_s(),
        };
        Asked { source, at_s, track }
    }

    /// Performs `target` driven by `input` at once on this thread if its lane has none waiting
    /// and it opens no file, else queues it on its lane.
    fn perform_or_queue<H: Host>(
        &self,
        host: &H,
        target: &Target,
        input: Input,
        asked: Asked,
        answer: Answer,
        ends: Ends,
    ) {
        let lane = lane(target);
        let waiting = lock(&self.lanes.0);
        if !reads_files(target) && !waiting.queued.contains_key(&lane) {
            drop(waiting);
            // a turn made already ends once it is performed
            let _ends = ends;
            return answer.give(host, perform_here(host, target, input, asked));
        }
        let ending = ends.made();
        let (host, target, asked) = (host.clone(), target.clone(), asked.into_owned());
        let job = move || {
            let _ending = ending;
            answer.give(&host, perform_here(&host, &target, input, asked));
        };
        self.lanes.push(waiting, lane, Box::new(job));
    }

    /// What `target` is sent as: a load on the first deck without a track, on that deck; a smart
    /// pad on an empty slot, a load (unless its slot has a load waiting, which may fill it); the
    /// library's go, what it does now (see [`browse::go`]).
    fn sent_as(&self, state: &AppState, target: &Target) -> Option<Target> {
        match *target {
            Target::Library(LibraryAction::LoadFree) => {
                Some(Target::Library(LibraryAction::Load(self.free_deck(state))))
            }
            Target::Library(LibraryAction::Go) => browse::go(state).map(Target::Library),
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
        let decks = state.decks_in_use();
        let tracks = lock(&state.tracks);
        (0..decks).find(|&deck| tracks[deck].is_none() && !self.lanes.busy(Lane::Track(deck))).unwrap_or(0)
    }

    /// Sends `send`, an action of the interface, once those it numbered before it are sent:
    /// their calls may reach Prisme in another order. One come is waited for as long as it
    /// takes; one that does not come is given up for lost once the lowest number waiting has
    /// waited [`ORDER_WAIT`], which then goes, the others still in turn after it.
    pub(crate) fn in_order<R>(&self, Order { run, number }: Order, send: impl FnOnce() -> R) -> R {
        let came = Instant::now();
        let mut sent = lock(&self.sent);
        if sent.run != run {
            *sent = Sent { run, next: number, ..Sent::default() };
            self.sent_on.notify_all();
        }
        sent.waiting.push(number);
        loop {
            // a page loaded since, or this one given up for lost and come late
            if sent.run != run || number < sent.next {
                break;
            }
            if number == sent.next && !sent.started {
                sent.started = true;
                break;
            }
            let lost = !sent.started && sent.waiting.iter().all(|&n| n >= number);
            sent = if lost {
                let waited = came.elapsed();
                if waited >= ORDER_WAIT {
                    (sent.next, sent.started) = (number, true);
                    break;
                }
                self.sent_on.wait_timeout(sent, ORDER_WAIT - waited).unwrap_or_else(|e| e.into_inner()).0
            } else {
                self.sent_on.wait(sent).unwrap_or_else(|e| e.into_inner())
            };
        }
        if sent.run == run {
            if let Some(at) = sent.waiting.iter().position(|&n| n == number) {
                sent.waiting.swap_remove(at);
            }
        }
        drop(sent);
        let _over = Sending { actions: self, order: Order { run, number } };
        send()
    }

    /// `job` done in turn on `lane`, after the actions waiting there (the Auto DJ's load of a
    /// deck, say).
    pub(crate) fn in_turn_on(&self, lane: Lane, job: impl FnOnce() + Send + 'static) {
        self.lanes.push(lock(&self.lanes.0), lane, Box::new(job));
    }

    /// Keeps in the library what `write` writes, in turn after what was kept before: the
    /// change is in memory and in the engine already.
    pub(crate) fn keep(&self, write: impl FnOnce() + Send + 'static) {
        // the worker lives as long as the application
        let _ = self.kept.send(Box::new(write));
    }

    /// A tap of the tempo of `deck` at `at_s`: the tempo, to a hundredth of a BPM, once there
    /// are enough taps.
    pub(crate) fn tapped(&self, deck: usize, at_s: f64) -> Option<f64> {
        let bpm = lock(&self.memory).decks[deck].taps.tap(at_s)?;
        Some((bpm * 100.0).round() / 100.0)
    }

    /// Whether the interface is told of `failure`, of an action a controller asked for: a
    /// refusal of a deck's record once for the messages of a gesture (see `Refusals`).
    fn tells(&self, failure: &AppError) -> bool {
        match *failure {
            AppError::RecordHolds { deck } => lock(&self.memory).refusals.tells(deck, self.now_s()),
            _ => true,
        }
    }

    /// Seconds since Prisme started: the clock of the actions no controller times.
    pub(crate) fn now_s(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// A page of the interface loaded: the keys the one before held are let go, nothing done.
    pub fn page_loaded(&self) {
        self.keys.clear();
    }

    /// The interface's keys held whose time came in `view`, performed (see `keys`): an action
    /// let go after it, its key let go meanwhile.
    pub fn fire_keys(&self, host: &impl Host, view: &View) {
        let source = || Source::Interface { confirmed: false };
        for (key, action) in self.keys.due(self.now_s(), view) {
            host.perform(&action, Input::Value(1.0), source(), &mut InTurn::default());
            if self.keys.fired(&key) {
                host.perform(&action, Input::Value(0.0), source(), &mut InTurn::default());
            }
        }
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

    /// [`Actions::view`] with the engine at `status`, its device laying the decks out as
    /// `layout` says (see [`view_with`]).
    pub fn view_with(&self, app: &AppHandle, status: EngineStatus, layout: Option<(usize, usize)>) -> View {
        let auto_dj = app.try_state::<AutoDj>().is_some_and(|a| a.plays());
        view_with(&app.state(), status, layout, auto_dj, &self.memory, self.now_s())
    }

    /// Waits, `within` at most, for the actions waiting their turn to be performed, then for
    /// what they changed to be kept in the library: whether they all were.
    pub fn finish(&self, within: Duration) -> bool {
        let waiting = Instant::now();
        while !lock(&self.lanes.0).queued.is_empty() {
            if waiting.elapsed() >= within {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let (kept, over) = mpsc::sync_channel(1);
        self.keep(move || {
            let _ = kept.send(());
        });
        over.recv_timeout(within.saturating_sub(waiting.elapsed())).is_ok()
    }

    /// Waits for the actions waiting their turn and what they keep, as long as a test may.
    #[cfg(test)]
    pub fn settle(&self) {
        assert!(self.finish(Duration::from_secs(10)), "the actions never settled");
    }
}

/// An action of the interface being sent (see [`Actions::in_order`]): the next one's turn
/// comes as it is over, whatever came of it.
struct Sending<'a> {
    actions: &'a Actions,
    order: Order,
}

impl Drop for Sending<'_> {
    fn drop(&mut self) {
        let Order { run, number } = self.order;
        let mut sent = lock(&self.actions.sent);
        if sent.run == run && sent.next == number && sent.started {
            (sent.next, sent.started) = (number + 1, false);
            self.actions.sent_on.notify_all();
        }
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
    fn perform_state(&self) -> MirroredGuard<'_, PerformState, PadsShown> {
        lock(&self.state.perform)
    }

    /// What the actions keep for themselves: let go before any other lock is taken.
    fn memory(&self) -> MutexGuard<'_, Memory> {
        lock(&self.actions.memory)
    }

    /// The Auto DJ, if it runs, lets go of what the DJ takes over by hand.
    fn take_over(&self, takeover: Takeover) {
        take_over(self.auto_dj().as_deref(), takeover);
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

    /// The track of the library it takes: the one selected as it was sent, or the one named.
    fn track(&self) -> Option<&std::path::Path> {
        self.asked.track.as_deref()
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

/// What the interface gives `target` as `value`: an encoder's steps, else a key's or a fader's
/// value.
fn interface_input(target: &Target, value: f32) -> Input {
    Input::of(value, target.kind() == Kind::Encoder, None)
}

impl Actions {
    /// Does `work` in its turn among the interface's actions (`order`), then waits for the
    /// answer it gives.
    pub fn answered(&self, order: Order, work: impl FnOnce(Answer) -> AppResult<()>) -> AppResult<()> {
        let (answer, outcome) = mpsc::sync_channel(1);
        self.in_order(order, || work(Answer::To(answer)))?;
        outcome.recv().unwrap_or_else(|e| Err(AppError::Other(e.to_string())))
    }

    /// Performs the action `target` driven to `value` (an encoder's steps, if it says so), as
    /// the interface asks: `confirmed`, the DJ said yes to what it replaces, the track it takes
    /// the one the DJ was asked about (`track`), else the one selected. The interface numbers
    /// its actions (`order`): each is sent in turn after those sent before it, a key let go
    /// after it was pressed; one that is not an action takes its turn all the same.
    pub fn perform_asked(
        &self,
        app: &AppHandle,
        target: &str,
        value: Option<f32>,
        confirmed: bool,
        track: Option<PathBuf>,
        order: Order,
    ) -> AppResult<()> {
        self.answered(order, |answer| {
            let target: Target = target.parse().map_err(|e: Unknown| AppError::Other(e.to_string()))?;
            let value = value.ok_or_else(|| AppError::Other(format!("{target} given no value")))?;
            let mut turn = InTurn::default();
            let mut send = |target: &Target, input, answer| {
                let asked = self.asked(Source::Interface { confirmed }, track.clone());
                self.send_asked(app, target, input, asked, answer, &mut turn);
            };
            match self.keys.press(&target, value, self.now_s()) {
                Press::Act => send(&target, interface_input(&target, value), answer),
                Press::Wait => answer.give(app, Ok(())),
                Press::Tap => {
                    send(&target, Input::Value(1.0), answer);
                    send(&target, Input::Value(0.0), Answer::Told);
                }
                Press::Release(hold) => send(&hold, Input::Value(0.0), answer),
                Press::Held(hold) => {
                    send(&hold, Input::Value(1.0), answer);
                    send(&hold, Input::Value(0.0), Answer::Told);
                }
            }
            Ok(())
        })
    }

    /// The interface's press of the key `target` lost (the key gone from under the pointer):
    /// let go, without what its release does, in its turn (`order`) after the press.
    pub fn lose_key(&self, target: &str, order: Order) -> AppResult<()> {
        self.in_order(order, || {
            let target: Target = target.parse().map_err(|e: Unknown| AppError::Other(e.to_string()))?;
            self.keys.lost(&target);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_interface_gives_an_encoder_steps_and_the_others_values() {
        let input = |id: &str, value| interface_input(&id.parse().unwrap(), value);
        assert!(matches!(input("deck1.loop.move", -1.0), Input::Steps { steps, .. } if steps == -1.0));
        assert!(matches!(input("library.browse", 3.0), Input::Steps { steps, .. } if steps == 3.0));
        assert!(matches!(input("deck1.eq.low", 0.25), Input::Value(v) if v == 0.25));
        assert!(matches!(input("deck1.play", 1.0), Input::Value(v) if v == 1.0));
    }
}

#[cfg(test)]
pub(crate) mod bench {
    //! An application whose engine no sound card calls: what an action sends it is read back.

    use super::*;
    use crate::audio_host::AudioSession;
    use crate::dto::{AudioInfo, AudioSettings, BackendDto};
    use crate::state::LoadedTrack;
    use prisme_engine::{Command, Engine, EngineConfig};
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
        /// Two decks, the audio running on an ASIO card with the turntables on it.
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
            let card = AudioSettings {
                backend: BackendDto::Asio,
                output_device: Some("Card".into()),
                input_device: Some("Card".into()),
                ..AudioSettings::default()
            };
            let state = AppState::new(JsonFile::in_memory(Default::default()));
            state.update_preferences(|p| p.audio = card.clone());
            let mut session = AudioSession::idle(controller, config, info);
            session.settings = card;
            *lock(&state.session) = Some(session);
            let library = Arc::new(LibraryDb::open_in_memory().unwrap());
            Self {
                state,
                actions: Actions::start(),
                library,
                engine: Mutex::new(engine),
                told: Mutex::default(),
            }
        }

        /// `path` selected in the library, listed alone.
        pub fn select(&self, path: &std::path::Path) {
            let path = path.display().to_string();
            self.state.browse(|l| {
                let rows = l.browser.entries([(path.clone(), None)]);
                l.browser.show(prisme_library::browse::List::Prepare, rows);
                let entry = l.browser.find(&path, None).expect("listed");
                l.browser.select(entry, prisme_library::browse::Pick::Only);
            });
        }

        /// Nothing selected in the library.
        pub fn unselect(&self) {
            self.state.browse(|l| l.browser.put_cursor(None));
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
            self.perform_taking(id, input, source, None)
        }

        /// `id` driven by `input` as `source` asks, taking `track` of the library if it names
        /// one, else the one selected.
        pub fn perform_taking(
            &self,
            id: &str,
            input: Input,
            source: Source,
            track: Option<PathBuf>,
        ) -> AppResult<()> {
            let target: Target = id.parse().unwrap();
            assert_eq!(target.owner(), Owner::Rust, "{id}");
            let target = self.actions.sent_as(&self.state, &target).unwrap_or(target);
            let selected = || takes_selection(&target).then(|| browse::selected(&self.state)).flatten();
            let track = track.or_else(selected);
            self.asked(self.actions.asked(source, track)).perform(&target, input)
        }

        /// An action taking no track of the library asked for by `source`, now.
        pub(in crate::actions) fn performing<'a>(&'a self, source: Source<'a>) -> Performing<'a> {
            let at_s = match &source {
                Source::Controller(moved) => moved.at_s,
                Source::Interface { .. } => self.actions.now_s(),
            };
            self.asked(Asked { source, at_s, track: None })
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
            let track = LoadedTrack::silent(&format!("deck{deck}.flac"), seconds, bpm);
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
            if target.owner() != Owner::Rust {
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

    fn split(bench: &Bench) -> bool {
        bench.state.preferences.read(|p| p.mixer.split_cue)
    }

    #[test]
    fn a_move_waiting_for_a_load_holds_up_no_other_lane() {
        let bench = Arc::new(Bench::new());
        let file = track("aside");
        bench.select(&file.0);
        let loading = held(&bench, Lane::Track(0));
        // a load, then the headphones split, on one key
        let in_turn = &mut InTurn::default();
        sent(&bench, "library.load1", 1.0, in_turn);
        sent(&bench, "mixer.headphones.split.on", 1.0, in_turn);
        let other = |value: f32| {
            let source =
                Source::Controller(Moved { device: "Other".into(), at_s: 0.0, jog_steps_per_turn: 720.0 });
            let crossfader: Target = "mixer.crossfader".parse().unwrap();
            let in_turn = &mut InTurn::default();
            bench.actions.perform(&bench, &crossfader, Input::Value(value), source, Answer::Told, in_turn);
        };
        other(0.5);
        other(0.75);
        assert_eq!(*lock(&bench.state.crossfader), 0.5, "another controller's crossfader at once");
        assert!(!split(&bench), "the split after the load");
        loading.send(()).unwrap();
        bench.actions.settle();
        assert!(lock(&bench.state.tracks)[0].is_some() && split(&bench));
    }

    #[test]
    fn a_controller_s_later_actions_on_a_lane_follow_the_one_it_set_aside_there() {
        let bench = Arc::new(Bench::new());
        let file = track("after-aside");
        bench.select(&file.0);
        let loading = held(&bench, Lane::Track(0));
        let in_turn = &mut InTurn::default();
        sent(&bench, "library.load1", 1.0, in_turn);
        sent(&bench, "mixer.headphones.split.on", 1.0, in_turn);
        sent(&bench, "mixer.headphones.split.off", 1.0, &mut InTurn::default());
        assert!(!split(&bench));
        loading.send(()).unwrap();
        bench.actions.settle();
        assert!(!split(&bench), "turned on, then off");
    }

    #[test]
    fn thousands_of_actions_set_aside_one_after_the_other_follow_without_deepening_the_stack() {
        let bench = Arc::new(Bench::new());
        let file = track("chain");
        bench.select(&file.0);
        let loading = held(&bench, Lane::Track(0));
        let in_turn = &mut InTurn::default();
        sent(&bench, "library.load1", 1.0, in_turn);
        sent(&bench, "mixer.headphones.split.on", 1.0, in_turn);
        for at in 0..20_000 {
            let id = if at % 2 == 0 { "mixer.headphones.split.off" } else { "mixer.headphones.split.on" };
            sent(&bench, id, 1.0, &mut InTurn::default());
        }
        assert!(!split(&bench), "every one waits for the load");
        loading.send(()).unwrap();
        bench.actions.settle();
        assert!(split(&bench), "the last, on, in turn");
    }

    #[test]
    fn quitting_waits_a_while_for_the_actions_waiting_and_what_they_keep() {
        let bench = Arc::new(Bench::new());
        let busy = held(&bench, Lane::Recording);
        assert!(!bench.actions.finish(Duration::from_millis(20)), "a lane busy");
        let (release, wait) = mpsc::sync_channel::<()>(1);
        bench.actions.keep(move || {
            let _ = wait.recv_timeout(Duration::from_secs(5));
        });
        busy.send(()).unwrap();
        assert!(!bench.actions.finish(Duration::from_millis(50)), "a write going on");
        release.send(()).unwrap();
        assert!(bench.actions.finish(Duration::from_secs(5)));
    }

    /// The bench, its actions failing by a bug.
    #[derive(Clone)]
    struct Panics(Arc<Bench>);

    impl Host for Panics {
        fn actions(&self) -> &Actions {
            &self.0.actions
        }

        fn app_state(&self) -> &AppState {
            &self.0.state
        }

        fn perform_here(&self, target: &Target, _: Input, _: Asked) -> AppResult<()> {
            panic!("{target} has a bug")
        }

        fn tell<P: Serialize + Clone>(&self, event: &str, payload: P) {
            self.0.tell(event, payload);
        }

        fn view(&self) -> View {
            self.0.view()
        }
    }

    #[test]
    fn an_action_that_panics_fails_alone_on_the_thread_asking_or_its_lane() {
        let bench = Arc::new(Bench::new());
        let panics = Panics(bench.clone());
        let source = || Source::Interface { confirmed: false };
        let perform = |id: &str| {
            let (answer, outcome) = mpsc::sync_channel(1);
            let target: Target = id.parse().unwrap();
            let in_turn = &mut InTurn::default();
            bench.actions.perform(&panics, &target, Input::Value(1.0), source(), Answer::To(answer), in_turn);
            outcome
        };
        assert!(perform("deck1.play").recv().unwrap().is_err(), "here");
        let busy = held(&bench, Lane::Track(0));
        let queued = perform("deck1.play");
        busy.send(()).unwrap();
        assert!(queued.recv().unwrap().is_err(), "on its lane");
        bench.actions.settle();
        pressed(&bench, "mixer.headphones.split.on");
        assert!(split(&bench), "the next goes on");
    }

    #[test]
    fn the_screen_plays_a_deck_while_the_library_reads_a_folder_and_loads_after_it() {
        let bench = Arc::new(Bench::new());
        bench.load(1, 60.0, None);
        let file = track("after-folder");
        let reading = held(&bench, Lane::Library);
        let screen = |id: &str| {
            let target: Target = id.parse().unwrap();
            let source = Source::Interface { confirmed: false };
            let in_turn = &mut InTurn::default();
            bench.actions.perform(&bench, &target, Input::Value(1.0), source, Answer::Told, in_turn);
        };
        screen("library.load1");
        screen("deck2.play");
        bench.run(0.05);
        assert!(bench.status(1).playing, "not held up by the folder");
        bench.select(&file.0);
        reading.send(()).unwrap();
        bench.actions.settle();
        let loaded = lock(&bench.state.tracks)[0].as_ref().map(|t| t.path.clone());
        assert_eq!(loaded, Some(file.0.clone()), "the row picked in the folder read");
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
    //! The loads of a controller, browsing in the library.

    use super::bench::{track, Bench, Track};
    use super::*;
    use std::time::Duration;

    fn pressed(bench: &Arc<Bench>, id: &str) {
        moved(bench, id, &mut InTurn::default());
    }

    fn loaded(bench: &Bench, deck: usize) -> Option<PathBuf> {
        lock(&bench.state.tracks)[deck].as_ref().map(|t| t.path.clone())
    }

    fn selected(bench: &Bench) -> Option<PathBuf> {
        browse::selected(&bench.state)
    }

    /// `id` pressed by the CDJ, after what `in_turn` sent.
    fn moved(bench: &Arc<Bench>, id: &str, in_turn: &mut InTurn) {
        let source = Source::Controller(Moved { device: "CDJ".into(), at_s: 0.0, jog_steps_per_turn: 720.0 });
        let target: Target = id.parse().unwrap();
        bench.actions.perform(bench, &target, Input::Value(1.0), source, Answer::Told, in_turn);
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
    fn a_load_waiting_its_turn_takes_the_track_selected_as_it_was_sent() {
        let bench = Arc::new(Bench::new());
        let (first, next) = (track("sent"), track("browsed-after"));
        bench.select(&first.0);
        let (release, wait) = mpsc::sync_channel::<()>(1);
        bench.actions.in_turn_on(Lane::Track(0), move || {
            let _ = wait.recv_timeout(Duration::from_secs(5));
        });
        pressed(&bench, "library.load1");
        bench.select(&next.0);
        release.send(()).unwrap();
        bench.actions.settle();
        assert_eq!(loaded(&bench, 0), Some(first.0.clone()));
    }

    #[test]
    fn a_controller_browses_then_loads_without_the_interface() {
        let bench = Arc::new(Bench::new());
        let files = [track("browse-a"), track("browse-b")];
        listed(&bench, &files);
        pressed(&bench, "library.next");
        pressed(&bench, "library.next");
        pressed(&bench, "library.load2");
        bench.actions.settle();
        assert_eq!(loaded(&bench, 1), Some(files[1].0.clone()), "the second row");
        assert!(bench.told().is_empty(), "the interface told nothing");
    }

    /// Two tracks listed in the library, nothing picked.
    fn listed(bench: &Bench, files: &[Track]) {
        bench.state.browse(|l| {
            let rows = l.browser.entries(files.iter().map(|f| (f.0.display().to_string(), None)));
            l.browser.show(prisme_library::browse::List::Prepare, rows);
        });
    }

    #[test]
    fn a_load_takes_the_row_the_library_s_actions_sent_before_it_leave() {
        let bench = Arc::new(Bench::new());
        let files = [track("left-a"), track("left-b")];
        listed(&bench, &files);
        let busy = held(&bench, Lane::Library);
        // a CDJ's track search loading what it finds, then another move's load
        let in_turn = &mut InTurn::default();
        for id in ["library.next", "library.load1"] {
            moved(&bench, id, in_turn);
        }
        moved(&bench, "library.next", &mut InTurn::default());
        moved(&bench, "library.load2", &mut InTurn::default());
        busy.send(()).unwrap();
        bench.actions.settle();
        assert_eq!(loaded(&bench, 0), Some(files[0].0.clone()), "the first row, browsed to");
        assert_eq!(loaded(&bench, 1), Some(files[1].0.clone()), "the second, browsed to after");
    }

    #[test]
    fn the_clock_s_looks_at_the_rows_hold_no_library_action() {
        let bench = Arc::new(Bench::new());
        let files = [track("looked-a"), track("looked-b")];
        listed(&bench, &files);
        let looking = held(&bench, Lane::Rows);
        pressed(&bench, "library.next");
        pressed(&bench, "library.next");
        pressed(&bench, "library.load1");
        assert_eq!(selected(&bench), Some(files[1].0.clone()), "browsed at once");
        looking.send(()).unwrap();
        bench.actions.settle();
        assert_eq!(loaded(&bench, 0), Some(files[1].0.clone()));
    }

    #[test]
    fn a_double_copies_the_track_the_deck_beside_was_loading() {
        let bench = Arc::new(Bench::new());
        let file = track("doubled");
        bench.select(&file.0);
        let loading = held(&bench, Lane::Track(0));
        pressed(&bench, "library.load1");
        pressed(&bench, "library.double2");
        loading.send(()).unwrap();
        bench.actions.settle();
        assert_eq!(loaded(&bench, 1), Some(file.0.clone()));
    }

    #[test]
    fn go_loads_the_track_selected_on_the_first_deck_not_heard() {
        let bench = Arc::new(Bench::new());
        let file = track("go");
        bench.load(0, 60.0, None);
        bench.press("deck1.play", 1.0).unwrap();
        bench.run(0.05);
        bench.select(&file.0);
        pressed(&bench, "library.go");
        bench.actions.settle();
        assert_eq!(loaded(&bench, 1), Some(file.0.clone()));
        bench.state.browse(|l| l.browser.set_focus(prisme_library::browse::Focus::Lists));
        assert_eq!(
            bench.actions.sent_as(&bench.state, &"library.go".parse().unwrap()),
            Some("library.list.open".parse().unwrap())
        );
    }
}

#[cfg(test)]
mod interface_order {
    use super::*;
    use std::time::Duration;

    /// Sends the interface's actions `orders` of `run`, each from a thread of its own a moment
    /// after the one before: the order they are performed in.
    fn sent(actions: &Arc<Actions>, run: u64, orders: &[u64]) -> Vec<u64> {
        let quick: Vec<_> = orders.iter().map(|&order| (order, 0)).collect();
        sent_taking(actions, run, &quick)
    }

    /// [`sent`], each action of `orders` taking so many milliseconds to send.
    fn sent_taking(actions: &Arc<Actions>, run: u64, orders: &[(u64, u64)]) -> Vec<u64> {
        let performed = Arc::new(Mutex::new(Vec::new()));
        let threads: Vec<_> = orders
            .iter()
            .map(|&(order, ms)| {
                let (actions, performed) = (actions.clone(), performed.clone());
                std::thread::sleep(Duration::from_millis(5));
                let numbered = Order { run, number: order };
                std::thread::spawn(move || {
                    actions.in_order(numbered, || {
                        std::thread::sleep(Duration::from_millis(ms));
                        lock(&performed).push(order);
                    })
                })
            })
            .collect();
        threads.into_iter().for_each(|t| t.join().unwrap());
        Arc::try_unwrap(performed).unwrap().into_inner().unwrap()
    }

    #[test]
    fn an_action_come_is_waited_for_however_long_it_takes() {
        let actions = Arc::new(Actions::start());
        assert_eq!(sent(&actions, 7, &[1]), [1]);
        assert_eq!(sent_taking(&actions, 7, &[(2, 120), (3, 0)]), [2, 3]);
    }

    #[test]
    fn of_the_actions_after_a_lost_one_the_lowest_goes_first() {
        let actions = Arc::new(Actions::start());
        assert_eq!(sent(&actions, 7, &[1]), [1]);
        // 2 lost, 4 (a release) come before 3 (its press)
        assert_eq!(sent(&actions, 7, &[4, 3]), [3, 4]);
    }

    #[test]
    fn a_call_that_fails_takes_its_turn() {
        let actions = Arc::new(Actions::start());
        assert_eq!(sent(&actions, 7, &[1]), [1]);
        let failed: Result<(), &str> = actions.in_order(Order { run: 7, number: 2 }, || Err("not an action"));
        assert!(failed.is_err());
        let next = Instant::now();
        assert_eq!(sent(&actions, 7, &[3]), [3]);
        assert!(next.elapsed() < ORDER_WAIT, "not waited for");
    }

    #[test]
    fn a_call_that_panics_takes_its_turn() {
        let actions = Arc::new(Actions::start());
        assert_eq!(sent(&actions, 7, &[1]), [1]);
        let bug = || actions.in_order(Order { run: 7, number: 2 }, || panic!("a bug"));
        assert!(catch_unwind(AssertUnwindSafe(bug)).is_err());
        let next = Instant::now();
        assert_eq!(sent(&actions, 7, &[3]), [3]);
        assert!(next.elapsed() < ORDER_WAIT, "not waited for");
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
