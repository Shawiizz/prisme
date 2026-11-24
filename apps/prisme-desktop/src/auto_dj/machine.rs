//! What the Auto DJ decides, apart from the app: from what the decks do and what heads the
//! prepare list, what to do next. [`Machine::tick`] runs at a steady pace; the DJ's requests
//! and gestures come in between.

use super::plan::{self, Audible, FadeLength, Incoming, MixPlan, Outgoing, MIN_SPEED};
use prisme_engine::BeatGrid;
use std::path::{Path, PathBuf};

/// The decks the Auto DJ plays on: A on the left of the crossfader, B on its right.
pub const DECKS: usize = 2;
/// The crossfader is moved again once it would go this far (a step too small to hear).
const CROSSFADER_STEP: f32 = 0.002;
/// A mix found late by less than this is caught up: the next track starts as far in as it
/// would be had it started on time. Later, it starts where it was meant to.
const CATCH_UP_MAX_S: f64 = 0.5;

/// A track on a deck, as the mix needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackView {
    pub path: PathBuf,
    /// Tells two loads apart, even of the same file.
    pub load_id: u64,
    pub grid: Option<BeatGrid>,
    pub cue_s: Option<f64>,
    /// `None` until its waveform is known.
    pub audible: Option<Audible>,
}

/// A deck, as the engine last saw it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeckView {
    pub playing: bool,
    pub position_s: f64,
    pub duration_s: f64,
    pub speed: f64,
    /// Its tempo now (with a beat grid).
    pub bpm: Option<f64>,
    /// It leads the sync: a deck snapped to it lands on its beats.
    pub leader: bool,
    /// It plays its track by itself: no record drives it, no input passes through it.
    pub free: bool,
    pub track: Option<TrackView>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub decks: [DeckView; DECKS],
    /// -1: deck A alone, 1: deck B alone.
    pub crossfader: f32,
    /// The software mixer is in use: its crossfader is heard.
    pub software_mixer: bool,
    /// The first track of the prepare list.
    pub next: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Load {
        deck: usize,
        path: PathBuf,
    },
    /// Puts a stopped deck where its track will start.
    Cue {
        deck: usize,
        position_s: f64,
    },
    /// Plays a deck from `from_s` at `rate`; `snap` lands it on the leader's beats.
    Start {
        deck: usize,
        from_s: f64,
        rate: f64,
        snap: bool,
    },
    Stop {
        deck: usize,
    },
    Crossfader(f32),
    /// Takes a track out of the prepare list: it starts, or is skipped.
    TakeOut(PathBuf),
}

/// What the DJ did by hand, that the Auto DJ gives way to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Takeover {
    /// Played, stopped, moved or jogged a deck.
    Transport(usize),
    Crossfader,
    Load(usize),
}

impl Takeover {
    fn concerns_the_auto_dj(self) -> bool {
        match self {
            Takeover::Transport(deck) | Takeover::Load(deck) => deck < DECKS,
            Takeover::Crossfader => true,
        }
    }
}

/// Why the Auto DJ stopped, paused, or left a track out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// An external mixer is in use: the Auto DJ cannot fade.
    ExternalMixer,
    /// A record drives deck A or B, or it passes its input through.
    Records,
    ListEmpty,
    /// The last track of the list came to its end.
    ListEnded,
    TakenOver(Takeover),
    /// A track of the list could not be loaded: it was left out.
    Unloadable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Off,
    On,
    Mixing,
    Paused,
}

/// What the Auto DJ shows of itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Status {
    pub state: State,
    pub reason: Option<Reason>,
    /// The deck heard.
    pub current: Option<usize>,
    /// The deck being mixed in.
    pub incoming: Option<usize>,
    /// Where on the deck heard the next mix starts.
    pub mix_at_s: Option<f64>,
}

/// A fade of the crossfader from where it was towards `incoming`, timed by the track there.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Mix {
    incoming: usize,
    from: f32,
    /// Where the incoming track was when the fade started, and how far it goes during it.
    start_s: f64,
    length_s: f64,
    /// The incoming deck has been seen playing (the engine tells it a moment after the start).
    heard: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Off,
    /// Turned on or resumed: takes the decks as they are at the next tick.
    Joining,
    /// Nothing played: the first track goes on `deck`, then plays.
    Starting {
        deck: usize,
    },
    /// `current` plays; the next track waits on the other deck. `heard`: it has been seen
    /// playing, so that a stop is the end of its track.
    Playing {
        current: usize,
        heard: bool,
    },
    Mixing {
        outgoing: usize,
        mix: Mix,
    },
    Paused,
}

pub struct Machine {
    phase: Phase,
    fade: FadeLength,
    reason: Option<Reason>,
    /// The load asked for, until it is done.
    loading: Option<(usize, PathBuf)>,
    /// A track that could not be loaded, to leave out of the list.
    failed: Option<PathBuf>,
    /// The load of the next track put where it will start (once).
    cued: Option<u64>,
    fade_now: bool,
    skip: bool,
    mix_at_s: Option<f64>,
    /// Where the Auto DJ last put the crossfader.
    crossfader: Option<f32>,
}

/// The end of the crossfader where `deck` is heard alone.
pub fn side(deck: usize) -> f32 {
    if deck == 0 {
        -1.0
    } else {
        1.0
    }
}

fn other(deck: usize) -> usize {
    1 - deck
}

impl Machine {
    pub fn new(fade: FadeLength) -> Self {
        Self {
            phase: Phase::Off,
            fade,
            reason: None,
            loading: None,
            failed: None,
            cued: None,
            fade_now: false,
            skip: false,
            mix_at_s: None,
            crossfader: None,
        }
    }

    pub fn fade(&self) -> FadeLength {
        self.fade
    }

    pub fn set_fade(&mut self, fade: FadeLength) {
        self.fade = fade;
    }

    /// Whether it is on, paused or not.
    pub fn is_on(&self) -> bool {
        self.phase != Phase::Off
    }

    pub fn status(&self) -> Status {
        let (state, current, incoming) = match self.phase {
            Phase::Off => (State::Off, None, None),
            Phase::Paused => (State::Paused, None, None),
            Phase::Joining => (State::On, None, None),
            Phase::Starting { deck } => (State::On, Some(deck), None),
            Phase::Playing { current, .. } => (State::On, Some(current), None),
            Phase::Mixing { outgoing, mix } => (State::Mixing, Some(outgoing), Some(mix.incoming)),
        };
        Status { state, reason: self.reason, current, incoming, mix_at_s: self.mix_at_s }
    }

    /// Off: on. Paused: on again, from the decks as they are. On: off, the decks left as they are.
    pub fn toggle(&mut self) {
        if self.phase == Phase::Off || self.phase == Phase::Paused {
            self.join();
        } else {
            self.stop(None);
        }
    }

    /// Mixes into the next track at once (resuming if paused).
    pub fn fade_now(&mut self) {
        match self.phase {
            Phase::Off | Phase::Mixing { .. } => {}
            Phase::Paused => {
                self.join();
                self.fade_now = true;
            }
            _ => self.fade_now = true,
        }
    }

    /// Leaves the first track of the list out: the one after comes next.
    pub fn skip(&mut self) {
        if self.is_on() {
            self.skip = true;
        }
    }

    /// The DJ acted by hand: the Auto DJ pauses rather than fight them.
    pub fn take_over(&mut self, takeover: Takeover) {
        let active = !matches!(self.phase, Phase::Off | Phase::Paused);
        if active && takeover.concerns_the_auto_dj() {
            self.phase = Phase::Paused;
            self.reason = Some(Reason::TakenOver(takeover));
            self.fade_now = false;
            self.mix_at_s = None;
        }
    }

    /// The load asked for is done (`loaded`) or failed.
    pub fn loaded(&mut self, deck: usize, path: &Path, loaded: bool) {
        if self.loading.as_ref().is_some_and(|(d, p)| *d == deck && p == path) {
            self.loading = None;
            if !loaded {
                self.failed = Some(path.to_path_buf());
            }
        }
    }

    pub fn tick(&mut self, view: &View) -> Vec<Action> {
        let mut actions = Vec::new();
        if self.phase == Phase::Off {
            return actions;
        }
        // what leaves the list is seen gone at the next tick
        if let Some(path) = self.failed.take() {
            self.reason = Some(Reason::Unloadable);
            actions.push(Action::TakeOut(path));
            return actions;
        }
        if std::mem::take(&mut self.skip) {
            if let Some(next) = &view.next {
                actions.push(Action::TakeOut(next.clone()));
                return actions;
            }
        }
        if self.phase == Phase::Paused {
            return actions;
        }
        if !view.software_mixer {
            self.stop(Some(Reason::ExternalMixer));
        } else if view.decks.iter().any(|d| !d.free) {
            self.stop(Some(Reason::Records));
        } else {
            match self.phase {
                Phase::Joining => self.settle(view, &mut actions),
                Phase::Starting { deck } => self.start(view, deck, &mut actions),
                Phase::Playing { current, heard } => self.play(view, current, heard, &mut actions),
                Phase::Mixing { outgoing, mix } => self.mix(view, outgoing, mix, &mut actions),
                Phase::Off | Phase::Paused => {}
            }
        }
        actions
    }

    fn join(&mut self) {
        self.phase = Phase::Joining;
        self.reason = None;
        self.fade_now = false;
        self.crossfader = None;
    }

    fn stop(&mut self, reason: Option<Reason>) {
        self.phase = Phase::Off;
        self.reason = reason;
        self.fade_now = false;
        self.skip = false;
        self.mix_at_s = None;
    }

    /// Takes the decks as the DJ leaves them: the one playing (the one the crossfader favours
    /// if both do) goes on, the crossfader brought over to it at the pace of a mix; with none
    /// playing, the first track of the list starts.
    fn settle(&mut self, view: &View, actions: &mut Vec<Action>) {
        let favoured = usize::from(view.crossfader > 0.0);
        let playing: Vec<usize> = (0..DECKS).filter(|&d| view.decks[d].playing).collect();
        let current = match playing[..] {
            [] => {
                self.phase = Phase::Starting { deck: favoured };
                return self.start(view, favoured, actions);
            }
            [only] => only,
            _ => favoured,
        };
        if view.crossfader == side(current) {
            self.phase = Phase::Playing { current, heard: true };
            return self.play(view, current, true, actions);
        }
        let deck = &view.decks[current];
        let seconds = match deck.bpm {
            Some(bpm) => f64::from(self.fade.beats) * 60.0 / bpm,
            None => self.fade.seconds,
        };
        let mix = Mix {
            incoming: current,
            from: view.crossfader,
            start_s: deck.position_s,
            length_s: seconds * deck.speed.abs().max(MIN_SPEED),
            heard: true,
        };
        self.phase = Phase::Mixing { outgoing: other(current), mix };
    }

    /// The first track, on `deck` alone.
    fn start(&mut self, view: &View, deck: usize, actions: &mut Vec<Action>) {
        let Some(next) = &view.next else {
            return self.stop(Some(Reason::ListEmpty));
        };
        let Some(track) = view.decks[deck].track.as_ref().filter(|t| t.path == *next) else {
            return self.load(deck, next, actions);
        };
        actions.extend([
            Action::Crossfader(side(deck)),
            Action::Start { deck, from_s: plan::start_of(&incoming(track)), rate: 1.0, snap: false },
            Action::TakeOut(next.clone()),
        ]);
        self.crossfader = Some(side(deck));
        self.cued = None;
        self.reason = None;
        self.phase = Phase::Playing { current: deck, heard: false };
    }

    fn play(&mut self, view: &View, current: usize, heard: bool, actions: &mut Vec<Action>) {
        let deck = &view.decks[current];
        let heard = heard || deck.playing;
        self.phase = Phase::Playing { current, heard };
        let next_deck = other(current);
        if heard && !deck.playing {
            // the track came to its end with nothing mixed in
            self.mix_at_s = None;
            if view.next.is_none() {
                return self.stop(Some(Reason::ListEnded));
            }
            self.phase = Phase::Starting { deck: next_deck };
            return self.start(view, next_deck, actions);
        }
        let Some(next) = &view.next else {
            self.mix_at_s = None;
            return;
        };
        if view.decks[next_deck].playing {
            return;
        }
        let Some(track) = view.decks[next_deck].track.as_ref().filter(|t| t.path == *next) else {
            self.mix_at_s = None;
            return self.load(next_deck, next, actions);
        };
        let incoming = incoming(track);
        if self.cued != Some(track.load_id) {
            self.cued = Some(track.load_id);
            actions.push(Action::Cue { deck: next_deck, position_s: plan::start_of(&incoming) });
        }
        if !heard {
            return;
        }
        let plan = plan::plan(&outgoing(deck), &incoming, self.fade);
        self.mix_at_s = Some(plan.at_s);
        if self.fade_now || deck.position_s >= plan.at_s {
            self.begin(view, current, next.clone(), plan, actions);
        }
    }

    /// Starts the next track and the fade into it.
    fn begin(
        &mut self,
        view: &View,
        current: usize,
        next: PathBuf,
        plan: MixPlan,
        actions: &mut Vec<Action>,
    ) {
        let deck = &view.decks[current];
        let late_s = (deck.position_s - plan.at_s) / deck.speed.abs().max(MIN_SPEED);
        let catch_up_s =
            if !self.fade_now && (0.0..CATCH_UP_MAX_S).contains(&late_s) { late_s * plan.rate } else { 0.0 };
        let incoming = other(current);
        let from_s = plan.from_s + catch_up_s;
        actions.extend([
            Action::Start { deck: incoming, from_s, rate: plan.rate, snap: plan.synced && deck.leader },
            Action::TakeOut(next),
        ]);
        let mix =
            Mix { incoming, from: view.crossfader, start_s: from_s, length_s: plan.length_s, heard: false };
        self.phase = Phase::Mixing { outgoing: current, mix };
        self.fade_now = false;
        self.mix_at_s = None;
        self.reason = None;
    }

    fn mix(&mut self, view: &View, outgoing: usize, mut mix: Mix, actions: &mut Vec<Action>) {
        let deck = &view.decks[mix.incoming];
        mix.heard |= deck.playing;
        self.phase = Phase::Mixing { outgoing, mix };
        if !mix.heard {
            return;
        }
        // a track ending during its own fade ends the fade
        let progress = if deck.playing && mix.length_s > 0.0 {
            ((deck.position_s - mix.start_s) / mix.length_s).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let to = side(mix.incoming);
        self.move_crossfader(mix.from + (to - mix.from) * progress as f32, to, actions);
        if progress >= 1.0 {
            if view.decks[outgoing].playing {
                actions.push(Action::Stop { deck: outgoing });
            }
            self.cued = None;
            self.phase = Phase::Playing { current: mix.incoming, heard: true };
        }
    }

    fn move_crossfader(&mut self, position: f32, end: f32, actions: &mut Vec<Action>) {
        let moved = match self.crossfader {
            Some(last) => (last - position).abs() >= CROSSFADER_STEP || (position == end && last != end),
            None => true,
        };
        if moved {
            self.crossfader = Some(position);
            actions.push(Action::Crossfader(position));
        }
    }

    fn load(&mut self, deck: usize, path: &Path, actions: &mut Vec<Action>) {
        if self.loading.is_none() {
            self.loading = Some((deck, path.to_path_buf()));
            actions.push(Action::Load { deck, path: path.to_path_buf() });
        }
    }
}

fn outgoing(deck: &DeckView) -> Outgoing {
    let track = deck.track.as_ref();
    Outgoing {
        grid: track.and_then(|t| t.grid),
        bpm: deck.bpm,
        end_s: track.and_then(|t| t.audible).map_or(deck.duration_s, |a| a.end_s.min(deck.duration_s)),
        speed: deck.speed,
    }
}

fn incoming(track: &TrackView) -> Incoming {
    Incoming {
        grid: track.grid,
        cue_s: track.cue_s,
        audible_start_s: track.audible.map_or(0.0, |a| a.start_s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FADE: FadeLength = FadeLength { beats: 16, seconds: 10.0 };
    const GRID: BeatGrid = BeatGrid { bpm: 120.0, first_beat_s: 0.0, bars_sure: true };

    fn path(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    fn track(name: &str, load_id: u64, grid: Option<BeatGrid>) -> TrackView {
        TrackView {
            path: path(name),
            load_id,
            grid,
            cue_s: None,
            audible: Some(Audible { start_s: 1.1, end_s: 198.0 }),
        }
    }

    fn deck(track: Option<TrackView>, playing: bool, position_s: f64) -> DeckView {
        DeckView {
            playing,
            position_s,
            duration_s: 210.0,
            speed: if playing { 1.0 } else { 0.0 },
            bpm: track.as_ref().and_then(|t| t.grid).map(|g| g.bpm),
            leader: playing,
            free: true,
            track,
        }
    }

    fn view(a: DeckView, b: DeckView, crossfader: f32, next: Option<&str>) -> View {
        View { decks: [a, b], crossfader, software_mixer: true, next: next.map(path) }
    }

    /// A machine on, with deck A playing `a.mp3` at `position_s` and `b.mp3` cued on deck B.
    fn playing_a(position_s: f64) -> (Machine, View) {
        let mut m = Machine::new(FADE);
        m.toggle();
        let v = view(
            deck(Some(track("a.mp3", 1, Some(GRID))), true, position_s),
            deck(Some(track("b.mp3", 2, Some(GRID))), false, 0.0),
            -1.0,
            Some("b.mp3"),
        );
        let actions = m.tick(&v);
        assert_eq!(actions, [Action::Cue { deck: 1, position_s: 1.5 }], "on its first beat heard");
        (m, v)
    }

    #[test]
    fn the_next_track_starts_on_the_last_phrase_and_in_time() {
        let (mut m, mut v) = playing_a(100.0);
        // heard until 198 s: 16 beats (8 s) fit from 190 s; the phrase before starts at 176 s
        assert_eq!(m.status().mix_at_s, Some(176.0));
        v.decks[0].position_s = 175.9;
        assert!(m.tick(&v).is_empty());
        // found 20 ms late: the next track starts 20 ms in, on the beats of the leader
        v.decks[0].position_s = 176.02;
        let actions = m.tick(&v);
        let Action::Start { deck: 1, from_s, rate, snap: true } = actions[0] else { panic!("{actions:?}") };
        assert!((from_s - 1.52).abs() < 1e-9 && rate == 1.0, "{from_s} {rate}");
        assert_eq!(actions[1], Action::TakeOut(path("b.mp3")));
        assert_eq!(m.status().state, State::Mixing);
    }

    #[test]
    fn the_crossfader_follows_the_incoming_track_then_the_outgoing_stops() {
        let (mut m, mut v) = playing_a(100.0);
        v.decks[0].position_s = 176.0;
        let actions = m.tick(&v);
        let Action::Start { from_s, .. } = actions[0] else { panic!() };
        // the engine has not started deck B yet: nothing moves
        assert!(m.tick(&v).is_empty());
        v.decks[1] = DeckView { playing: true, speed: 1.0, position_s: from_s, ..v.decks[1].clone() };
        assert_eq!(m.tick(&v), [Action::Crossfader(-1.0)]);
        // halfway through the 8 s fade, the middle
        v.decks[1].position_s = from_s + 4.0;
        assert_eq!(m.tick(&v), [Action::Crossfader(0.0)]);
        assert!(m.tick(&v).is_empty(), "no step too small to hear");
        v.decks[1].position_s = from_s + 8.5;
        assert_eq!(m.tick(&v), [Action::Crossfader(1.0), Action::Stop { deck: 0 }]);
        assert_eq!(m.status().state, State::On);
        assert_eq!(m.status().current, Some(1));
    }

    #[test]
    fn the_next_track_is_loaded_on_the_free_deck() {
        let mut m = Machine::new(FADE);
        m.toggle();
        let a = deck(Some(track("a.mp3", 1, None)), true, 10.0);
        let v = view(a.clone(), deck(None, false, 0.0), -1.0, Some("b.mp3"));
        assert_eq!(m.tick(&v), [Action::Load { deck: 1, path: path("b.mp3") }]);
        assert!(m.tick(&v).is_empty(), "loaded once");
        m.loaded(1, &path("b.mp3"), true);
        let v = view(a, deck(Some(track("b.mp3", 2, None)), false, 0.0), -1.0, Some("b.mp3"));
        assert_eq!(m.tick(&v), [Action::Cue { deck: 1, position_s: 1.1 }]);
        // without grids: 10 s before the end of what is heard
        assert_eq!(m.status().mix_at_s, Some(188.0));
    }

    #[test]
    fn a_reordered_list_puts_its_new_first_track_on_the_deck() {
        let (mut m, mut v) = playing_a(100.0);
        v.next = Some(path("c.mp3"));
        assert_eq!(m.tick(&v), [Action::Load { deck: 1, path: path("c.mp3") }]);
    }

    #[test]
    fn with_nothing_playing_the_first_track_starts_alone() {
        let mut m = Machine::new(FADE);
        m.toggle();
        let v = view(deck(None, false, 0.0), deck(None, false, 0.0), 0.3, Some("a.mp3"));
        assert_eq!(
            m.tick(&v),
            [Action::Load { deck: 1, path: path("a.mp3") }],
            "the side the crossfader favours"
        );
        m.loaded(1, &path("a.mp3"), true);
        let v =
            view(deck(None, false, 0.0), deck(Some(track("a.mp3", 1, None)), false, 0.0), 0.3, Some("a.mp3"));
        assert_eq!(
            m.tick(&v),
            [
                Action::Crossfader(1.0),
                Action::Start { deck: 1, from_s: 1.1, rate: 1.0, snap: false },
                Action::TakeOut(path("a.mp3")),
            ]
        );
        // the engine still tells it stopped: that is not its end
        let v = View { next: None, ..v };
        assert!(m.tick(&v).is_empty());
        assert_eq!(m.status().state, State::On);
    }

    #[test]
    fn the_dj_acting_pauses_it_until_turned_on_again() {
        let (mut m, v) = playing_a(100.0);
        m.take_over(Takeover::Transport(2));
        assert_eq!(m.status().state, State::On, "deck C is not the Auto DJ's");
        m.take_over(Takeover::Crossfader);
        let status = m.status();
        assert_eq!(
            (status.state, status.reason),
            (State::Paused, Some(Reason::TakenOver(Takeover::Crossfader)))
        );
        let mut late = v.clone();
        late.decks[0].position_s = 190.0;
        assert!(m.tick(&late).is_empty(), "no mix while paused");
        m.toggle();
        assert_eq!(m.status().reason, None);
        assert!(matches!(m.tick(&late)[0], Action::Start { deck: 1, .. }), "resumed where the decks are");
    }

    #[test]
    fn a_pause_during_a_mix_resumes_by_finishing_it_at_the_pace_of_a_fade() {
        let (mut m, mut v) = playing_a(100.0);
        v.decks[0].position_s = 176.0;
        m.tick(&v);
        m.take_over(Takeover::Load(1));
        assert_eq!(m.status().reason, Some(Reason::TakenOver(Takeover::Load(1))));
        // the DJ left both decks playing, the crossfader leaning towards B
        v.decks[1] = deck(Some(track("b.mp3", 2, Some(GRID))), true, 10.0);
        v.crossfader = 0.5;
        m.toggle();
        assert!(m.tick(&v).is_empty());
        assert_eq!((m.status().current, m.status().incoming), (Some(0), Some(1)));
        v.decks[1].position_s = 14.0;
        assert_eq!(m.tick(&v), [Action::Crossfader(0.75)], "half of 16 beats");
        v.decks[1].position_s = 18.0;
        assert_eq!(m.tick(&v), [Action::Crossfader(1.0), Action::Stop { deck: 0 }]);
    }

    #[test]
    fn fade_now_mixes_at_once_and_skip_takes_the_track_after() {
        let (mut m, v) = playing_a(30.0);
        m.skip();
        assert_eq!(m.tick(&v), [Action::TakeOut(path("b.mp3"))]);
        let v = View { next: Some(path("c.mp3")), ..v };
        assert_eq!(m.tick(&v), [Action::Load { deck: 1, path: path("c.mp3") }]);
        m.loaded(1, &path("c.mp3"), true);
        let mut v = v;
        v.decks[1].track = Some(track("c.mp3", 3, Some(GRID)));
        m.fade_now();
        let actions = m.tick(&v);
        assert_eq!(actions[0], Action::Cue { deck: 1, position_s: 1.5 });
        assert_eq!(
            actions[1],
            Action::Start { deck: 1, from_s: 1.5, rate: 1.0, snap: true },
            "not caught up"
        );
        assert_eq!(actions[2], Action::TakeOut(path("c.mp3")));
    }

    #[test]
    fn a_track_that_cannot_be_loaded_is_left_out() {
        let mut m = Machine::new(FADE);
        m.toggle();
        let v = view(
            deck(Some(track("a.mp3", 1, None)), true, 10.0),
            deck(None, false, 0.0),
            -1.0,
            Some("x.mp3"),
        );
        m.tick(&v);
        m.loaded(1, &path("x.mp3"), false);
        assert_eq!(m.tick(&v), [Action::TakeOut(path("x.mp3"))]);
        assert_eq!(m.status().reason, Some(Reason::Unloadable));
        assert_eq!(m.status().state, State::On);
    }

    #[test]
    fn it_stops_once_the_list_is_played() {
        let mut m = Machine::new(FADE);
        m.toggle();
        let a = deck(Some(track("a.mp3", 1, None)), true, 150.0);
        let v = view(a.clone(), deck(None, false, 0.0), -1.0, None);
        assert!(m.tick(&v).is_empty());
        assert_eq!(m.status().mix_at_s, None, "nothing to mix into");
        let ended = DeckView { playing: false, position_s: 210.0, ..a };
        m.tick(&view(ended, deck(None, false, 0.0), -1.0, None));
        assert_eq!((m.status().state, m.status().reason), (State::Off, Some(Reason::ListEnded)));
    }

    #[test]
    fn a_track_ending_unmixed_hands_over_to_the_next_at_once() {
        let (mut m, mut v) = playing_a(100.0);
        v.decks[0] = DeckView { playing: false, position_s: 210.0, ..v.decks[0].clone() };
        let actions = m.tick(&v);
        assert_eq!(actions[0], Action::Crossfader(1.0));
        assert!(matches!(actions[1], Action::Start { deck: 1, rate, snap: false, .. } if rate == 1.0));
    }

    #[test]
    fn it_needs_the_software_mixer_and_decks_playing_by_themselves() {
        let mut m = Machine::new(FADE);
        m.toggle();
        let (_, v) = playing_a(10.0);
        m.tick(&View { software_mixer: false, ..v.clone() });
        assert_eq!((m.status().state, m.status().reason), (State::Off, Some(Reason::ExternalMixer)));
        m.toggle();
        let mut records = v;
        records.decks[1].free = false;
        m.tick(&records);
        assert_eq!(m.status().reason, Some(Reason::Records));
    }

    #[test]
    fn an_empty_list_has_nothing_to_start() {
        let mut m = Machine::new(FADE);
        m.toggle();
        m.tick(&view(deck(None, false, 0.0), deck(None, false, 0.0), 0.0, None));
        assert_eq!((m.status().state, m.status().reason), (State::Off, Some(Reason::ListEmpty)));
    }
}
