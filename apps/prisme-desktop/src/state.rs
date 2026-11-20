//! Application state shared by the commands: the preferences, the audio session, the
//! loaded tracks and the deck settings (kept so they can be re-applied when the audio
//! restarts).

use crate::actions::browse::Library;
use crate::audio_host::{deck_pair, layer_partner, AudioSession};
use crate::dto::{
    AudioInfo, BackendDto, BandDto, ChannelStripDto, CrossfaderSideDto, CueDto, DeckStemsDto, FxChainDto,
    FxPartDto, HeadphonesOutputDto, InputChannelsDto, KeyDto, LiveSourceDto, MixModeDto, PreviewStateDto,
    RpmDto, SeparationDto, StemMixDto, TempoDto, TrackInfoDto, VinylModeDto,
};
use crate::error::{AppError, AppResult};
use crate::folders;
use crate::preferences::{DeckPreferences, LivePreferences, MixerPreferences, Preferences};
use crate::recording::Recorder;
use crate::sampler::Samplers;
use crate::track_waveform::TrackWaveform;
use prisme_actions::perform::PerformState;
use prisme_analysis::TrackAnalysis;
use prisme_engine::{
    BeatGrid, Command, DeckStatus, EngineConfig, EngineController, EngineStatus, LoopRegion, MixMode,
    OutputRouting, StemTracks, MAX_DECKS,
};
use prisme_library::{Cue, CueSlot, TrackEdits};
use prisme_media::{TrackAudio, TrackTags};
use prisme_session::JsonFile;
use prisme_timecode::TimecodeFormat;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tracing::warn;

/// The loudness auto gain brings every track to: that of a loud modern master, so that most
/// tracks come down a little and few need a boost.
pub const AUTO_GAIN_LUFS: f64 = -10.0;

/// The gain bringing a track of `loudness` to [`AUTO_GAIN_LUFS`]: at most 12 dB down and 6 dB
/// up, and never so far up that its peak would clip.
fn auto_gain_for(loudness: prisme_analysis::Loudness) -> f32 {
    let db = (AUTO_GAIN_LUFS - loudness.lufs).clamp(-12.0, 6.0);
    let gain = 10f64.powf(db / 20.0) as f32;
    if gain > 1.0 {
        gain.min(1.0 / loudness.peak.max(1e-6)).max(1.0)
    } else {
        gain
    }
}

#[derive(Clone)]
pub struct LoadedTrack {
    /// Tells two loads apart, even of the same file.
    pub load_id: u64,
    pub path: PathBuf,
    pub audio: Arc<TrackAudio>,
    pub waveform: TrackWaveform,
    /// As detected; [`LoadedTrack::analysis`] applies the corrections.
    pub detected: TrackAnalysis,
    pub edits: TrackEdits,
    pub tags: TrackTags,
    pub cues: Vec<Cue>,
    /// Semitones the deck transposes it by (0 again at each load).
    pub key_shift: i8,
    /// Its separated stems, once read (`prisme_stems::track`).
    pub stems: StemTracks,
    /// How the deck plays each part (all at full level again at each load).
    pub stem_mix: StemMixDto,
    /// How loud the track is, once its audio is all decoded.
    pub loudness: Option<prisme_analysis::Loudness>,
}

impl LoadedTrack {
    pub fn next_load_id() -> u64 {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }

    /// A silent track of `seconds` named `path`, with a beat grid at `bpm` from its start if it
    /// has one.
    #[cfg(test)]
    pub fn silent(path: &str, seconds: f64, bpm: Option<f64>) -> Self {
        let samples = vec![0.0; (seconds * 48_000.0) as usize * 2];
        let tempo = bpm.map(|bpm| prisme_analysis::Tempo { bpm, first_beat_s: 0.0, confidence: 1.0 });
        LoadedTrack {
            load_id: Self::next_load_id(),
            path: path.into(),
            audio: Arc::new(TrackAudio::from_interleaved(48_000, samples)),
            waveform: crate::track_waveform::TrackWaveform::growing(1),
            detected: prisme_analysis::TrackAnalysis { tempo, key: None },
            edits: Default::default(),
            tags: Default::default(),
            cues: Vec::new(),
            key_shift: 0,
            stems: Default::default(),
            stem_mix: Default::default(),
            loudness: None,
        }
    }

    /// The same track as a load of its own, its audio and stems shared: an instant double.
    pub fn double(&self) -> Self {
        Self { load_id: Self::next_load_id(), ..self.clone() }
    }

    /// What the UI shows of the track on `deck`.
    pub fn info(&self, deck: usize) -> TrackInfoDto {
        let analysis = self.analysis();
        TrackInfoDto {
            deck,
            path: self.path.display().to_string(),
            title: self.tags.title.clone().unwrap_or_else(|| folders::title_from_path(&self.path)),
            artist: self.tags.artist.clone(),
            duration_s: self.audio.duration_s(),
            sample_rate: self.audio.sample_rate(),
            load_id: self.load_id,
            tempo: analysis.tempo.as_ref().map(TempoDto::from),
            key: analysis.key.as_ref().map(KeyDto::from),
            grid_edited: self.edits.grid.is_some(),
            key_edited: self.edits.key.is_some(),
            cues: self.cue_dtos(),
            key_shift: self.key_shift,
            stems: DeckStemsDto::of(&self.stems),
            stem_mix: self.stem_mix,
        }
    }

    pub fn analysis(&self) -> TrackAnalysis {
        self.edits.apply(self.detected)
    }

    pub fn grid(&self) -> Option<BeatGrid> {
        self.analysis().tempo.map(|t| BeatGrid { bpm: t.bpm, first_beat_s: t.first_beat_s })
    }

    /// Commands that put the track, with its beat grid, key shift and stems, on `deck`, at
    /// its level if `auto_gain`.
    pub fn load_commands(&self, deck: usize, auto_gain: bool) -> [Command; 6] {
        [
            Command::LoadTrack { deck, track: self.audio.clone() },
            Command::SetBeatGrid { deck, grid: self.grid() },
            Command::SetKeyShift { deck, semitones: self.key_shift },
            Command::SetStems { deck, stems: self.stems.clone() },
            Command::SetStemMix { deck, mix: self.stem_mix.into(), on_bar: false },
            Command::SetLevel { deck, gain: self.level(auto_gain) },
        ]
    }

    /// Its gain: the one bringing it to [`AUTO_GAIN_LUFS`] if `auto_gain`, else 1.
    pub fn level(&self, auto_gain: bool) -> f32 {
        self.loudness.filter(|_| auto_gain).map_or(1.0, auto_gain_for)
    }

    /// How far it is separated: the stems it has.
    pub fn separation(&self) -> SeparationDto {
        SeparationDto::of(|stem| self.stems[stem as usize].is_some())
    }

    pub fn cue(&self, slot: CueSlot) -> Option<&Cue> {
        self.cues.iter().find(|c| c.slot == slot)
    }

    /// Adds `cue`, replacing the one in the same slot.
    pub fn put_cue(&mut self, cue: Cue) {
        self.remove_cue(cue.slot);
        self.cues.push(cue);
    }

    pub fn remove_cue(&mut self, slot: CueSlot) {
        self.cues.retain(|c| c.slot != slot);
    }

    pub fn cue_dtos(&self) -> Vec<CueDto> {
        self.cues.iter().map(CueDto::from).collect()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DeckSettings {
    pub pitch: f64,
    pub volume: f32,
    pub cue: bool,
    pub vinyl_enabled: bool,
    pub vinyl_mode: VinylModeDto,
    pub rpm: RpmDto,
    /// Chosen, else the record's own.
    pub lead_in_s: Option<f64>,
    pub swap_channels: bool,
    /// The record on its turntable.
    pub timecode: &'static TimecodeFormat,
    pub key_lock: bool,
    /// Software mixer: the channel fader starts and stops the deck.
    pub fader_start: bool,
    /// The range of its tempo fader, if it has one of its own.
    pub pitch_range: Option<f64>,
    pub strip: ChannelStripDto,
    pub fx: FxChainDto,
    pub sampler_gain: f32,
    /// Its size in beats: what its jumps go by, what its sized loop lasts.
    pub size_beats: f64,
    /// The deck plays its input instead of its track.
    pub passthrough: bool,
    /// Its side of the crossfader, if the DJ chose another than its letter's.
    pub side: Option<CrossfaderSideDto>,
    /// A touch of its controller's jog wheel holds the record.
    pub jog_scratch: bool,
    /// At the end, the track goes on from its start.
    pub repeat: bool,
    /// Silenced in the master, after its fader.
    pub mute: bool,
}

impl From<DeckPreferences> for DeckSettings {
    fn from(p: DeckPreferences) -> Self {
        Self {
            pitch: 1.0,
            volume: 1.0,
            cue: false,
            vinyl_enabled: p.vinyl_enabled,
            vinyl_mode: p.vinyl_mode,
            rpm: p.rpm,
            lead_in_s: p.lead_in_s,
            swap_channels: p.swap_channels,
            timecode: p.timecode_format(),
            key_lock: p.key_lock,
            fader_start: p.fader_start,
            pitch_range: p.pitch_range,
            strip: ChannelStripDto::default(),
            fx: FxChainDto::default(),
            sampler_gain: p.sampler_level(),
            size_beats: p.size(),
            passthrough: false,
            side: None,
            jog_scratch: true,
            repeat: false,
            mute: false,
        }
    }
}

/// Where the track of a deck is and how it plays: what an audio restart carries over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    /// The load it is of: a track loaded meanwhile is not put there.
    pub load_id: u64,
    pub position_s: f64,
    pub playing: bool,
    /// The loop stored, and whether it plays; none under a roll, which the restart lets go.
    pub looping: Option<(LoopRegion, bool)>,
    pub sync_locked: bool,
    /// Whether it led the others, chosen or not.
    pub leader: bool,
    pub slip: bool,
    pub reverse: bool,
}

impl Place {
    fn of(load_id: u64, d: &DeckStatus) -> Self {
        let looping = d.looping.region.filter(|_| !d.looping.rolling).map(|r| (r, d.looping.active));
        Place {
            load_id,
            position_s: d.position_s,
            playing: d.playing,
            looping,
            sync_locked: d.sync.locked,
            leader: d.sync.leader,
            slip: d.slip_mode,
            reverse: d.reverse,
        }
    }

    /// What puts `deck` back there.
    fn commands(&self, deck: usize) -> impl Iterator<Item = Command> {
        let looping =
            self.looping.map(|(region, active)| Command::SetLoop { deck, region: Some(region), active });
        let synced = self.sync_locked.then_some(Command::SetSyncLocked { deck, locked: true });
        let leader = self.leader.then_some(Command::ResumeSyncLeader { deck });
        [Command::Seek { deck, seconds: self.position_s }, Command::SetSlip { deck, enabled: self.slip }]
            .into_iter()
            .chain(looping)
            .chain([
                Command::SetReverse { deck, enabled: self.reverse },
                Command::SetPlaying { deck, playing: self.playing },
            ])
            .chain(synced)
            .chain(leader)
    }
}

/// A live source's switches as the DJ left them (off at each start).
#[derive(Debug, Clone, Copy, Default)]
pub struct LiveSwitches {
    /// Heard in the master.
    pub on: bool,
    /// Heard in the headphones.
    pub cue: bool,
}

pub struct AppState {
    /// Changed by [`AppState::update_preferences`] only, which the interface hears of.
    pub preferences: JsonFile<Preferences>,
    pub session: Mutex<Option<AudioSession>>,
    pub tracks: Mirrored<[Option<LoadedTrack>; MAX_DECKS]>,
    load_turns: LoadTurns,
    pub decks: Mirrored<[DeckSettings; MAX_DECKS]>,
    pub crossfader: Mirrored<f32>,
    /// The deck chosen to lead the others; `None`: the engine picks it.
    pub sync_leader: Mutex<Option<usize>>,
    /// The microphone's and the aux line's, in the order of [`LiveSourceDto::ALL`].
    pub live: Mirrored<[LiveSwitches; 2]>,
    pub recorder: Mutex<Option<Recorder>>,
    /// Held while the audio starts: the watchdog and the user never start it at once.
    pub audio_start: Mutex<()>,
    /// Where the decks were as the audio last stopped to start again: kept for the start
    /// after it, should that one fail or its engine never run.
    carried: Mutex<[Option<Place>; MAX_DECKS]>,
    pub samplers: Mirrored<Samplers>,
    /// What the actions keep between presses (see `prisme_actions::perform`).
    pub perform: Mirrored<PerformState>,
    /// What the headphones preview, as the interface shows it (see `preview::Previews`).
    pub preview: Mirrored<PreviewStateDto>,
    /// The library as it is browsed, changed through [`AppState::browse`]: taken last, nothing
    /// else locked while it is held.
    pub library: Mutex<Library>,
    /// The interface is out of sight: a phone's application in the background, which the
    /// system may stop calling for audio.
    pub hidden: AtomicBool,
    /// Moved by every change of what the interface mirrors (see [`Mirrored`]).
    revisions: Arc<Revisions>,
}

/// A part of what the interface mirrors (`ControlStateDto`), sent again alone when it changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// The decks' settings, their samplers, the mixer, the routing, the DJ's preferences.
    Mixer,
    /// The decks' tracks, and their loads.
    Tracks,
    /// What the pads hold.
    Perform,
    Preview,
    /// The list browsed, as the interface shows it.
    Library,
    /// The lists the library keeps: the crates, the sets, the prepare list.
    Lists,
}

impl Section {
    pub const ALL: [Section; SECTIONS] = [
        Section::Mixer,
        Section::Tracks,
        Section::Perform,
        Section::Preview,
        Section::Library,
        Section::Lists,
    ];
}

pub const SECTIONS: usize = 6;

/// Where the changes of each [`Section`] are at: each moves at every change of its own.
#[derive(Default)]
struct Revisions([AtomicU64; SECTIONS]);

impl Revisions {
    fn of(&self, section: Section) -> &AtomicU64 {
        &self.0[section as usize]
    }

    fn moved(&self, section: Section) {
        self.of(section).fetch_add(1, Ordering::SeqCst);
    }
}

/// State the interface mirrors (`ControlStateDto`): taken to be changed, it moves the
/// revision of its section, by which the status stream tells the interface of a change.
pub struct Mirrored<T> {
    value: Mutex<T>,
    revisions: Arc<Revisions>,
    section: Section,
}

impl<T> Mirrored<T> {
    fn new(value: T, revisions: &Arc<Revisions>, section: Section) -> Self {
        Self { value: Mutex::new(value), revisions: revisions.clone(), section }
    }
}

/// A [`Mirrored`] locked: read freely, changed through it alone.
pub struct MirroredGuard<'a, T> {
    guard: MutexGuard<'a, T>,
    revision: &'a AtomicU64,
}

impl<T> Deref for MirroredGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.guard
    }
}

/// Moved while the lock is held: a revision read before the change sees its value once it can
/// lock it.
impl<T> DerefMut for MirroredGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.revision.fetch_add(1, Ordering::SeqCst);
        &mut self.guard
    }
}

/// What [`lock`] locks: a mutex, or state the interface mirrors.
pub trait Lock {
    type Guard<'a>
    where
        Self: 'a;

    fn guard(&self) -> Self::Guard<'_>;
}

impl<T> Lock for Mutex<T> {
    type Guard<'a>
        = MutexGuard<'a, T>
    where
        T: 'a;

    fn guard(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl<L: Lock> Lock for Arc<L> {
    type Guard<'a>
        = L::Guard<'a>
    where
        L: 'a;

    fn guard(&self) -> L::Guard<'_> {
        (**self).guard()
    }
}

impl<T> Lock for Mirrored<T> {
    type Guard<'a>
        = MirroredGuard<'a, T>
    where
        T: 'a;

    fn guard(&self) -> MirroredGuard<'_, T> {
        MirroredGuard { guard: self.value.guard(), revision: self.revisions.of(self.section) }
    }
}

/// The loads asked for on each deck, in turn: one ends on its deck only if no other was asked
/// for there since. A deck loads until the last one asked for there is over.
#[derive(Default)]
struct LoadTurns {
    asked: [AtomicU64; MAX_DECKS],
    /// The last turn over on each deck.
    over: [AtomicU64; MAX_DECKS],
}

impl LoadTurns {
    fn ask(&self, deck: usize) -> u64 {
        self.asked[deck].fetch_add(1, Ordering::SeqCst) + 1
    }

    fn is_last(&self, deck: usize, turn: u64) -> bool {
        self.asked[deck].load(Ordering::SeqCst) == turn
    }

    fn end(&self, deck: usize, turn: u64) {
        self.over[deck].fetch_max(turn, Ordering::SeqCst);
    }

    fn loading(&self, deck: usize) -> bool {
        self.over[deck].load(Ordering::SeqCst) != self.asked[deck].load(Ordering::SeqCst)
    }
}

/// A load asked for on a deck ([`AppState::ask_load`]), over once dropped, whether its track
/// went there or not.
pub struct LoadTurn<'a> {
    state: &'a AppState,
    deck: usize,
    turn: u64,
}

impl Drop for LoadTurn<'_> {
    fn drop(&mut self) {
        self.state.load_turns.end(self.deck, self.turn);
        self.state.revisions.moved(Section::Tracks);
    }
}

/// The speed (1: as recorded) from which a track counts as moving: a record at rest
/// trembles under its needle.
const MOVING_SPEED: f64 = 0.05;

/// How long a restore waits in all for the engine to take its commands: longer than its
/// periods, however long they are, and than its stream may take to start calling it.
const RESTORE_WAIT: Duration = Duration::from_secs(1);
/// How often a full queue is looked at again.
const ROOM_POLL: Duration = Duration::from_millis(1);

/// Locks a mutex, recovering from a poisoned lock (a panicking command must not take
/// the whole application down).
pub fn lock<L: Lock>(l: &L) -> L::Guard<'_> {
    l.guard()
}

impl AppState {
    pub fn new(preferences: JsonFile<Preferences>) -> Self {
        if preferences.read(|p| p.clone().settle()) {
            preferences.update(|p| p.settle());
        }
        let saved = preferences.get().decks;
        let revisions = Arc::new(Revisions::default());
        Self {
            preferences,
            session: Mutex::new(None),
            tracks: Mirrored::new(Default::default(), &revisions, Section::Tracks),
            load_turns: LoadTurns::default(),
            decks: Mirrored::new(saved.map(DeckSettings::from), &revisions, Section::Mixer),
            crossfader: Mirrored::new(0.0, &revisions, Section::Mixer),
            sync_leader: Mutex::default(),
            live: Mirrored::new(Default::default(), &revisions, Section::Mixer),
            recorder: Mutex::new(None),
            audio_start: Mutex::new(()),
            carried: Mutex::default(),
            samplers: Mirrored::new(Samplers::default(), &revisions, Section::Mixer),
            perform: Mirrored::new(PerformState::default(), &revisions, Section::Perform),
            preview: Mirrored::new(PreviewStateDto::default(), &revisions, Section::Preview),
            library: Mutex::default(),
            hidden: AtomicBool::new(false),
            revisions,
        }
    }

    /// Where the changes of what the interface mirrors are at, by [`Section`]: each moves at
    /// every change of its own.
    pub fn revision(&self) -> [u64; SECTIONS] {
        Section::ALL.map(|section| self.revisions.of(section).load(Ordering::SeqCst))
    }

    /// The sections that moved since they were at `before` (a [`AppState::revision`]).
    #[cfg(test)]
    pub fn moved_since(&self, before: [u64; SECTIONS]) -> Vec<Section> {
        let now = self.revision();
        Section::ALL.into_iter().filter(|&s| now[s as usize] != before[s as usize]).collect()
    }

    /// Whether `deck` is one the engine can have, kept for when it is in use (its settings);
    /// [`AppState::deck_in_use`] for one in use.
    pub fn deck_exists(deck: usize) -> AppResult<()> {
        if deck < MAX_DECKS {
            Ok(())
        } else {
            Err(AppError::InvalidDeck(deck))
        }
    }

    /// Changes the library as it is browsed: the interface hears of what changed.
    pub fn browse<R>(&self, change: impl FnOnce(&mut Library) -> R) -> R {
        let mut library = lock(&self.library);
        let before = (library.browser.revision(), library.browser.lists_revision());
        let changed = change(&mut library);
        if library.browser.revision() != before.0 {
            self.revisions.moved(Section::Library);
        }
        if library.browser.lists_revision() != before.1 {
            self.revisions.moved(Section::Lists);
        }
        changed
    }

    /// The track at `path` now separated as far as `separation`: its rows say it.
    pub fn separated(&self, path: &str, separation: SeparationDto) {
        lock(&self.library).separated(path, separation);
        self.revisions.moved(Section::Library);
    }

    /// Changes the preferences, saved soon after; the interface hears of it.
    pub fn update_preferences(&self, change: impl FnOnce(&mut Preferences)) {
        self.preferences.update(change);
        self.revisions.moved(Section::Mixer);
    }

    /// The engine's latest view of `deck`.
    /// The audio running on `session` from now, or stopped (`None`): the one it replaces is
    /// given back, to be closed outside the lock. The interface hears of the decks and the
    /// routing as the device lays them out.
    pub fn replace_session(&self, session: Option<AudioSession>) -> Option<AudioSession> {
        let previous = std::mem::replace(&mut *lock(&self.session), session);
        self.revisions.moved(Section::Mixer);
        previous
    }

    /// What the running engine shows of every deck.
    pub fn engine_status(&self) -> AppResult<EngineStatus> {
        let mut session = lock(&self.session);
        Ok(session.as_mut().ok_or(AppError::AudioNotRunning)?.controller.status())
    }

    pub fn deck_status(&self, deck: usize) -> AppResult<DeckStatus> {
        Self::deck_exists(deck)?;
        let mut session = lock(&self.session);
        let session = session.as_mut().ok_or(AppError::AudioNotRunning)?;
        Ok(session.controller.status().decks[deck])
    }

    /// Where the track of each deck is: as the running engine shows it once it has run with
    /// what its start sent it, else as it was carried into that start.
    pub fn places(&self) -> [Option<Place>; MAX_DECKS] {
        let status = lock(&self.session).as_mut().map(|s| (s.controller.status(), s.restored));
        let loads = lock(&self.tracks).each_ref().map(|t| t.as_ref().map(|t| t.load_id));
        places_of(status, loads, *lock(&self.carried))
    }

    /// [`AppState::places`], kept as the audio stops to start again.
    pub fn carry_places(&self) -> [Option<Place>; MAX_DECKS] {
        let places = self.places();
        *lock(&self.carried) = places;
        places
    }

    /// Whether `deck` plays: its track moves, by itself or under a record.
    pub fn deck_plays(&self, deck: usize) -> bool {
        self.deck_status(deck).is_ok_and(|d| moves(&d))
    }

    /// Whether a deck plays: its track moves, by itself or under a record.
    pub fn is_playing(&self) -> bool {
        let mut session = lock(&self.session);
        let Some(session) = session.as_mut() else { return false };
        let status = session.controller.status();
        status.decks[..status.deck_count].iter().any(moves)
    }

    /// Reads the track loaded on `deck` with `f`.
    pub fn with_track<R>(&self, deck: usize, f: impl FnOnce(&LoadedTrack) -> R) -> AppResult<R> {
        Self::deck_exists(deck)?;
        lock(&self.tracks)[deck].as_ref().map(f).ok_or(AppError::NoTrack(deck))
    }

    /// Changes the track loaded on `deck` with `f`: the interface hears of it.
    pub fn change_track<R>(&self, deck: usize, f: impl FnOnce(&mut LoadedTrack) -> R) -> AppResult<R> {
        Self::deck_exists(deck)?;
        let mut tracks = lock(&self.tracks);
        if tracks[deck].is_none() {
            return Err(AppError::NoTrack(deck));
        }
        tracks[deck].as_mut().map(f).ok_or(AppError::NoTrack(deck))
    }

    /// Reads the track on `deck` with `f` if it is still the load `load_id`.
    pub fn with_loaded<R>(&self, deck: usize, load_id: u64, f: impl FnOnce(&LoadedTrack) -> R) -> Option<R> {
        lock(&self.tracks).get(deck)?.as_ref().filter(|t| t.load_id == load_id).map(f)
    }

    /// Changes the track on `deck` with `f` if it is still the load `load_id`.
    pub fn change_loaded<R>(
        &self,
        deck: usize,
        load_id: u64,
        f: impl FnOnce(&mut LoadedTrack) -> R,
    ) -> Option<R> {
        let mut tracks = lock(&self.tracks);
        tracks.get(deck)?.as_ref().filter(|t| t.load_id == load_id)?;
        tracks[deck].as_mut().map(f)
    }

    /// The decks playing `audio`, and their loads (see [`AppState::change_loads_of`]).
    pub fn loads_of(&self, audio: &Arc<TrackAudio>) -> Vec<(usize, u64)> {
        let tracks = lock(&self.tracks);
        let playing = tracks.iter().enumerate().filter_map(|(deck, t)| Some((deck, t.as_ref()?)));
        playing.filter(|(_, t)| Arc::ptr_eq(&t.audio, audio)).map(|(deck, t)| (deck, t.load_id)).collect()
    }

    /// Changes with `f` every loaded track playing `audio` (a load and its instant doubles):
    /// what is found out about a load goes to them all. The deck, load and result of each.
    pub fn change_loads_of<R>(
        &self,
        audio: &Arc<TrackAudio>,
        f: impl FnMut(&mut LoadedTrack) -> R,
    ) -> Vec<(usize, u64, R)> {
        self.change_tracks(|t| Arc::ptr_eq(&t.audio, audio), f)
    }

    /// Changes with `f` every loaded track of the file `path` (loaded twice, or doubled): what
    /// is changed of a file changes on every deck playing it. The deck, load and result of each.
    pub fn change_file<R>(&self, path: &Path, f: impl FnMut(&mut LoadedTrack) -> R) -> Vec<(usize, u64, R)> {
        self.change_tracks(|t| t.path == path, f)
    }

    /// Changes with `f` the tracks `which` takes: the interface hears of it if there are any.
    fn change_tracks<R>(
        &self,
        which: impl Fn(&LoadedTrack) -> bool,
        mut f: impl FnMut(&mut LoadedTrack) -> R,
    ) -> Vec<(usize, u64, R)> {
        let mut tracks = lock(&self.tracks);
        if !tracks.iter().flatten().any(&which) {
            return Vec::new();
        }
        tracks
            .iter_mut()
            .enumerate()
            .filter_map(|(deck, t)| {
                let t = t.as_mut().filter(|t| which(t))?;
                Some((deck, t.load_id, f(t)))
            })
            .collect()
    }

    /// Asks for a load on `deck` (a track, a double, or none): those asked for before it no
    /// longer end there. Its turn, for [`AppState::put_track`]: the deck loads until it is
    /// dropped, if no other was asked for there since.
    pub fn ask_load(&self, deck: usize) -> LoadTurn<'_> {
        let turn = self.load_turns.ask(deck);
        self.revisions.moved(Section::Tracks);
        LoadTurn { state: self, deck, turn }
    }

    /// Whether each deck loads a track.
    pub fn loading(&self) -> [bool; MAX_DECKS] {
        std::array::from_fn(|deck| self.load_turns.loading(deck))
    }

    /// Puts `track` on the deck of `turn` and sends the engine `commands` (its load), unless
    /// another load was asked for there since; whether it did.
    pub fn put_track(
        &self,
        turn: &LoadTurn,
        track: Option<LoadedTrack>,
        commands: impl IntoIterator<Item = Command>,
    ) -> AppResult<bool> {
        // held while the engine is told: it gets the loads in the order the decks do
        let mut tracks = lock(&self.tracks);
        if !self.load_turns.is_last(turn.deck, turn.turn) {
            return Ok(false);
        }
        tracks[turn.deck] = track;
        for c in commands {
            self.send_if_running(c)?;
        }
        Ok(true)
    }

    /// Sends a command to the running engine.
    pub fn send(&self, command: Command) -> AppResult<()> {
        let mut session = lock(&self.session);
        let session = session.as_mut().ok_or(AppError::AudioNotRunning)?;
        session.controller.send(command).map_err(|_| AppError::EngineBusy)
    }

    /// Sends a command if the audio runs; without audio it will be applied by
    /// [`AppState::restore`] when the audio starts.
    pub fn send_if_running(&self, command: Command) -> AppResult<()> {
        match self.send(command) {
            Ok(()) | Err(AppError::AudioNotRunning) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// `change` made to the settings of `deck`, kept as the audio stops, then `command` sent to
    /// the engine if it runs.
    pub fn set_deck(
        &self,
        deck: usize,
        change: impl FnOnce(&mut DeckSettings),
        command: Command,
    ) -> AppResult<()> {
        Self::deck_exists(deck)?;
        change(&mut lock(&self.decks)[deck]);
        self.send_if_running(command)
    }

    /// `change` made to the mixer's preferences, kept for the next start, then `command` sent
    /// to the engine if it runs.
    pub fn set_mixer(&self, change: impl FnOnce(&mut MixerPreferences), command: Command) -> AppResult<()> {
        self.update_preferences(|p| change(&mut p.mixer));
        self.send_if_running(command)
    }

    /// Puts every loaded track at its level, after auto gain was turned on or off.
    pub fn apply_levels(&self) -> AppResult<()> {
        let auto_gain = self.preferences.read(|p| p.dj.auto_gain);
        let levels: Vec<(usize, f32)> = lock(&self.tracks)
            .iter()
            .enumerate()
            .filter_map(|(deck, t)| Some((deck, t.as_ref()?.level(auto_gain))))
            .collect();
        for (deck, gain) in levels {
            self.send_if_running(Command::SetLevel { deck, gain })?;
        }
        Ok(())
    }

    /// Sample rate of the running audio.
    pub fn sample_rate(&self) -> Option<u32> {
        lock(&self.session).as_ref().map(|s| s.engine_config.sample_rate)
    }

    /// The range of the tempo fader of `deck`: its own, else the general one.
    pub fn tempo_range(&self, deck: usize) -> f64 {
        self.preferences.read(|p| p.tempo_range(deck))
    }

    /// Whether the software mixer runs: the channel faders are Prisme's.
    pub fn mixes_itself(&self) -> bool {
        let info = lock(&self.session).as_ref().map(|s| s.info.clone());
        info.is_some_and(|info| self.routing(&info).mode == MixMode::Internal)
    }

    /// The routing for the running device (see [`routing_for`]).
    pub fn routing(&self, info: &AudioInfo) -> OutputRouting {
        routing_for(self.preferences.read(|p| p.mixer), info)
    }

    /// The first output the preview is heard on: the headphones, on the running device, if
    /// they are apart from what the audience hears.
    pub fn preview_output(&self) -> AppResult<usize> {
        let info = lock(&self.session).as_ref().map(|s| s.info.clone()).ok_or(AppError::AudioNotRunning)?;
        self.routing(&info).preview_output(info.decks).ok_or(AppError::NoPreviewOutput)
    }

    /// Where `source` comes in on the device of `info`: the inputs chosen, if the turntables
    /// leave them free.
    pub fn live_input(&self, source: LiveSourceDto, info: Option<&AudioInfo>) -> Option<InputChannelsDto> {
        let chosen = self.preferences.read(|p| live_preferences(&p.mixer, source).input);
        live_channels(chosen, &free_inputs(info?))
    }

    /// Commands that put the master's balance, the booth and the live sources as the DJ left
    /// them, on the device of `info`.
    pub fn live_commands(&self, info: Option<&AudioInfo>) -> Vec<Command> {
        let mixer = self.preferences.read(|p| p.mixer);
        let switches = *lock(&self.live);
        let mut commands = vec![
            Command::SetMasterGain { gain: mixer.main_gain },
            Command::SetBalance { balance: mixer.balance },
            Command::SetBoothGain { gain: mixer.booth_gain },
            Command::SetTalkover { enabled: mixer.talkover, depth_db: mixer.talkover_depth_db },
        ];
        for (source, switches) in LiveSourceDto::ALL.into_iter().zip(switches) {
            let channels = self.live_input(source, info).map(Into::into);
            let gain = live_preferences(&mixer, source).level;
            let source = source.into();
            commands.extend([
                Command::SetLiveInput { source, channels },
                Command::SetLiveLevel { source, gain },
                Command::SetLiveOn { source, on: switches.on },
                Command::SetLiveCue { source, cue: switches.cue },
            ]);
        }
        commands
    }

    /// Gives the turntable of `deck` to it alone: the deck layered with it on the running
    /// device, if a record drove it, goes on by itself. Returns that deck.
    pub fn take_turntable(&self, deck: usize) -> Option<usize> {
        let partner = lock(&self.session)
            .as_ref()
            .and_then(|s| layer_partner(deck, s.info.decks, s.info.input_channels))?;
        if !std::mem::take(&mut lock(&self.decks)[partner].vinyl_enabled) {
            return None;
        }
        self.update_preferences(|p| p.decks[partner].vinyl_enabled = false);
        self.let_go_of_record(partner);
        let _ = self.send_if_running(Command::SetVinylEnabled { deck: partner, enabled: false });
        Some(partner)
    }

    /// The simulated turntable in the audio starting: as it comes, it takes deck A, its needle
    /// 10 s into the record where the track starts; one that `ran` already leaves deck A as
    /// the DJ chose.
    pub fn simulate_turntable(&self, ran: bool) {
        if !ran {
            let mut decks = lock(&self.decks);
            decks[0].vinyl_enabled = true;
            decks[0].lead_in_s = Some(prisme_timecode::testing::SimulatedTurntable::START_S);
        }
    }

    /// Before the vinyl control of `deck` is turned off: the engine has it play on at the
    /// tempo its record gave it, its tempo fader's from then on. Whether a record drove it.
    pub fn let_go_of_record(&self, deck: usize) -> bool {
        let Ok(status) = self.deck_status(deck) else { return false };
        if status.vinyl.enabled {
            lock(&self.decks)[deck].pitch = status.rate;
        }
        status.vinyl.enabled
    }

    /// Gives the decks layered on one turntable of the device of `info` its one record (see
    /// [`share_records`]), kept for the next start.
    pub fn share_turntable_records(&self, info: &AudioInfo) {
        let mut decks = lock(&self.decks);
        let changed = share_records(&mut decks, info);
        let records = decks.map(|d| d.timecode.id);
        drop(decks);
        if !changed.is_empty() {
            self.update_preferences(|p| {
                for deck in changed {
                    p.decks[deck].timecode = Some(records[deck].into());
                }
            });
        }
    }

    /// The decks and input channels of the running device (see [`AudioInfo::layout`]).
    pub fn layout(&self) -> Option<(usize, usize)> {
        lock(&self.session).as_ref().map(|s| s.info.layout())
    }

    /// The settings of the decks as a device of so many decks and inputs (`layout`, none
    /// without audio) lays them out, as the interface and the lights show them: of two decks
    /// layered on a turntable and both asked to follow it, the first does. What was asked
    /// stays, for a device with a turntable each.
    pub fn decks_laid_out(&self, layout: Option<(usize, usize)>) -> [DeckSettings; MAX_DECKS] {
        let mut decks = *lock(&self.decks);
        let Some((count, inputs)) = layout else { return decks };
        for deck in 0..count {
            let first = layer_partner(deck, count, inputs).filter(|&partner| partner < deck);
            if first.is_some_and(|first| decks[first].vinyl_enabled) {
                decks[deck].vinyl_enabled = false;
            }
        }
        decks
    }

    /// Whether `deck` is one of the decks in use (two, or four).
    pub fn deck_in_use(&self, deck: usize) -> AppResult<()> {
        if deck < self.preferences.read(|p| p.dj.decks()) {
            Ok(())
        } else {
            Err(AppError::InvalidDeck(deck))
        }
    }

    /// Re-applies tracks, deck and mixer settings to a freshly started engine, the tracks at
    /// their `places`.
    pub fn restore(&self, session: &mut AudioSession, places: &[Option<Place>; MAX_DECKS]) {
        let commands = self.restore_commands(&session.info, &session.engine_config, places);
        send_all(&mut session.controller, commands, RESTORE_WAIT);
        session.restored = session.controller.sent();
    }

    /// What [`AppState::restore`] sends, for the decks of `info` only.
    fn restore_commands(
        &self,
        info: &AudioInfo,
        config: &EngineConfig,
        places: &[Option<Place>; MAX_DECKS],
    ) -> Vec<Command> {
        let auto_gain = self.preferences.read(|p| p.dj.auto_gain);
        let decks = self.decks_laid_out(Some(info.layout()));
        let mut commands = Vec::new();
        for (deck, (track, settings)) in lock(&self.tracks).iter().zip(&decks).enumerate().take(info.decks) {
            commands.extend(track.iter().flat_map(|t| t.load_commands(deck, auto_gain)));
            commands.extend(deck_commands(deck, settings));
            // the record may have changed while the engine started
            if settings.swap_channels || settings.timecode.id != config.timecodes[deck].id {
                let decoder = config.decoder(settings.timecode, settings.swap_channels);
                commands.push(Command::ReplaceDecoder { deck, decoder });
            }
        }
        for (deck, slots) in lock(&self.samplers).iter().enumerate().take(info.decks) {
            for (slot, loaded) in slots.iter().enumerate() {
                if let Some(loaded) = loaded {
                    commands.push(Command::SetSample { deck, slot, sample: Some(loaded.audio.clone()) });
                }
            }
        }
        let preferences = self.preferences.get();
        commands.extend([
            Command::SetCrossfader { position: *lock(&self.crossfader) },
            Command::SetSyncLeader { deck: *lock(&self.sync_leader) },
            Command::SetQuantize { enabled: preferences.dj.quantize },
            Command::SetRouting { routing: self.routing(info) },
            Command::SetCueMix { mix: preferences.mixer.cue_mix },
            Command::SetHeadphoneGain { gain: preferences.mixer.headphone_gain },
            Command::SetSplitCue { enabled: preferences.mixer.split_cue },
            Command::SetCrossfaderCurve { curve: preferences.mixer.crossfader_curve.into() },
            Command::SetLimiter { enabled: preferences.mixer.limiter },
            Command::SetCrossfaderReverse { enabled: preferences.mixer.crossfader_reverse },
        ]);
        commands.extend(self.live_commands(Some(info)));
        for (deck, place) in places.iter().enumerate().take(info.decks) {
            commands.extend(place.iter().flat_map(|place| place.commands(deck)));
        }
        commands
    }
}

/// Sends `commands` in turn, waiting for the engine to make room in its queue (it takes them
/// at each period) for at most `wait` in all: an engine that does not run gets no more.
fn send_all(controller: &mut EngineController, commands: Vec<Command>, wait: Duration) {
    let deadline = Instant::now() + wait;
    let count = commands.len();
    for (sent, mut command) in commands.into_iter().enumerate() {
        while let Err(refused) = controller.send(command) {
            if Instant::now() >= deadline {
                warn!("the engine took {sent} of the {count} commands restoring it");
                return;
            }
            command = refused;
            std::thread::sleep(ROOM_POLL);
        }
    }
}

/// Where the tracks of `loads` (their load on each deck) are: as the engine's status shows
/// them once it follows the first commands sent to it (those restoring the decks), else as
/// `carried`.
fn places_of(
    status: Option<(EngineStatus, u64)>,
    loads: [Option<u64>; MAX_DECKS],
    carried: [Option<Place>; MAX_DECKS],
) -> [Option<Place>; MAX_DECKS] {
    let status = status.filter(|(status, restored)| status.commands >= *restored).map(|(status, _)| status);
    std::array::from_fn(|deck| {
        let load_id = loads[deck]?;
        match &status {
            Some(status) => (deck < status.deck_count).then(|| Place::of(load_id, &status.decks[deck])),
            None => carried[deck].filter(|place| place.load_id == load_id),
        }
    })
}

pub fn live_preferences(mixer: &MixerPreferences, source: LiveSourceDto) -> &LivePreferences {
    match source {
        LiveSourceDto::Mic => &mixer.mic,
        LiveSourceDto::Aux => &mixer.aux,
    }
}

/// The input channels of the device of `info` the turntables do not take (0-based): where a
/// microphone or a line can come in.
pub fn free_inputs(info: &AudioInfo) -> Vec<usize> {
    let taken: Vec<usize> =
        (0..info.decks).map(|deck| 2 * deck_pair(deck, info.decks, info.input_channels)).collect();
    (0..info.input_channels).filter(|c| !taken.contains(&(c - c % 2))).collect()
}

/// The channels chosen for a live source, if they are all `free`.
fn live_channels(chosen: Option<InputChannelsDto>, free: &[usize]) -> Option<InputChannelsDto> {
    chosen.filter(|c| c.channels().all(|channel| free.contains(&channel)))
}

/// A deck's track moves, by itself or under a record.
fn moves(d: &DeckStatus) -> bool {
    d.loaded && d.speed.abs() > MOVING_SPEED
}

/// Decks layered on one turntable of the device of `info` (see `layer_partner`) read its one
/// record: that of the deck that has the turntable, the one on the record, else the first.
/// Returns the decks whose record changed.
pub fn share_records(decks: &mut [DeckSettings; MAX_DECKS], info: &AudioInfo) -> Vec<usize> {
    let mut changed = Vec::new();
    for first in 0..info.decks {
        let Some(second) = layer_partner(first, info.decks, info.input_channels).filter(|&d| d > first)
        else {
            continue;
        };
        let holder = if decks[second].vinyl_enabled && !decks[first].vinyl_enabled { second } else { first };
        let record = decks[holder].timecode;
        for deck in [first, second] {
            if decks[deck].timecode.id != record.id {
                decks[deck].timecode = record;
                changed.push(deck);
            }
        }
    }
    changed
}

/// Commands that put a deck in the state described by `s`.
pub fn deck_commands(deck: usize, s: &DeckSettings) -> Vec<Command> {
    let strip = s.strip;
    let side = s.side.unwrap_or(CrossfaderSideDto::of_deck(deck)).into();
    let mut commands = vec![
        Command::SetCrossfaderSide { deck, side },
        Command::SetDeckCue { deck, enabled: s.cue },
        Command::SetPitch { deck, ratio: s.pitch },
        Command::SetVolume { deck, gain: s.volume },
        Command::SetVinylMode { deck, mode: s.vinyl_mode.into() },
        Command::SetRpm { deck, rpm: s.rpm.into() },
        Command::SetLeadIn { deck, seconds: s.lead_in_s },
        Command::SetVinylEnabled { deck, enabled: s.vinyl_enabled },
        Command::SetKeyLock { deck, enabled: s.key_lock },
        Command::SetTrim { deck, gain: strip.trim },
        Command::SetFilter { deck, position: strip.filter },
        Command::SetFxParts { deck, parts: FxPartDto::parts(s.fx.part) },
        Command::SetFxChainEnabled { deck, enabled: s.fx.enabled },
        Command::SetSamplerGain { deck, gain: s.sampler_gain },
        Command::SetPassthrough { deck, enabled: s.passthrough },
        Command::SetRepeat { deck, enabled: s.repeat },
        Command::SetMute { deck, muted: s.mute },
    ];
    for (slot, fx) in s.fx.slots.iter().enumerate() {
        commands.extend([
            Command::SetFx { deck, slot, kind: fx.kind.into() },
            Command::SetFxMix { deck, slot, mix: fx.mix },
            Command::SetFxAmount { deck, slot, amount: fx.amount },
            Command::SetFxParam2 { deck, slot, param2: fx.param2 },
            Command::SetFxBeats { deck, slot, beats: fx.beats },
            Command::SetFxEnabled { deck, slot, enabled: fx.enabled },
        ]);
    }
    for (i, band) in BandDto::ALL.into_iter().enumerate() {
        commands.push(Command::SetEq { deck, band: band.into(), gain: strip.eq[i] });
        commands.push(Command::SetEqKill { deck, band: band.into(), kill: strip.kills[i] });
    }
    commands
}

/// The routing `mixer` asks for on the device of `info`: an external mixer needs two output
/// pairs (four decks share them if there are no more, see `deck_pair`), its headphones a pair
/// of their own for the preview, after the decks' on an ASIO interface by default; else the
/// software mixer is used, its headphones and booth where the DJ wants them.
fn routing_for(mixer: MixerPreferences, info: &AudioInfo) -> OutputRouting {
    let outputs = info.output_channels;
    let deck_outputs = std::array::from_fn(|deck| Some(2 * deck_pair(deck, info.decks, outputs)));
    if mixer.mode != MixModeDto::External || outputs < 4 {
        let headphones = headphones_output(mixer.headphones, info.backend, outputs);
        let booth = booth_output(mixer.booth, headphones, outputs);
        return OutputRouting {
            mode: MixMode::Internal,
            deck_outputs,
            headphones,
            booth,
            ..Default::default()
        };
    }
    let after_decks = 2 * (deck_pair(info.decks - 1, info.decks, outputs) + 1);
    let headphones = match mixer.headphones {
        HeadphonesOutputDto::Auto => (info.backend == BackendDto::Asio).then_some(after_decks),
        HeadphonesOutputDto::Off => None,
        HeadphonesOutputDto::Pair { first } => Some(first),
    };
    let routing = OutputRouting {
        mode: MixMode::External,
        deck_outputs,
        headphones: headphones.filter(|first| first + 2 <= outputs),
        ..Default::default()
    };
    OutputRouting { headphones: routing.preview_output(info.decks), ..routing }
}

/// The first output of the headphones on a device with `outputs`, reached through
/// `backend`: the pair chosen if the device has it, by default 3-4 of a DJ interface (four
/// outputs, or ASIO) and none on the surround outputs of a computer, where 3-4 are the
/// centre and the subwoofer of the room.
fn headphones_output(choice: HeadphonesOutputDto, backend: BackendDto, outputs: usize) -> Option<usize> {
    let first = match choice {
        HeadphonesOutputDto::Auto => (outputs == 4 || backend == BackendDto::Asio).then_some(2),
        HeadphonesOutputDto::Off => None,
        HeadphonesOutputDto::Pair { first } => Some(first),
    };
    first.filter(|first| first + 2 <= outputs)
}

/// The first output of the booth on a device with `outputs`: the pair chosen if the device has
/// it, apart from the master (1-2) and the headphones.
fn booth_output(chosen: Option<usize>, headphones: Option<usize>, outputs: usize) -> Option<usize> {
    chosen.filter(|&first| {
        first >= 2 && first + 2 <= outputs && headphones.is_none_or(|h| first.abs_diff(h) >= 2)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use prisme_analysis::Loudness;
    use prisme_engine::CrossfaderSide::{Left, Right};

    #[test]
    fn the_decks_are_on_either_side_of_the_crossfader() {
        let side = |deck| {
            deck_commands(deck, &DeckSettings::from(DeckPreferences::default())).into_iter().find_map(
                |command| match command {
                    Command::SetCrossfaderSide { side, .. } => Some(side),
                    _ => None,
                },
            )
        };
        assert_eq!([0, 1, 2, 3].map(side), [Left, Right, Left, Right].map(Some));
    }

    #[test]
    fn the_headphones_stay_off_the_surround_channels_of_a_computer() {
        use BackendDto::{Asio, Wasapi};
        use HeadphonesOutputDto::{Auto, Off, Pair};
        assert_eq!(headphones_output(Auto, Wasapi, 4), Some(2), "a DJ interface");
        assert_eq!(headphones_output(Auto, Wasapi, 2), None);
        assert_eq!(headphones_output(Auto, Wasapi, 6), None, "5.1");
        assert_eq!(headphones_output(Auto, Wasapi, 8), None, "7.1");
        assert_eq!(headphones_output(Auto, Asio, 10), Some(2));
        assert_eq!(headphones_output(Auto, Asio, 2), None);
        assert_eq!(headphones_output(Pair { first: 4 }, Wasapi, 8), Some(4), "chosen on purpose");
        assert_eq!(headphones_output(Pair { first: 4 }, Wasapi, 4), None, "not on this device");
        assert_eq!(headphones_output(Off, Wasapi, 4), None);
    }

    #[test]
    fn with_an_external_mixer_the_headphones_take_a_pair_no_deck_uses() {
        use BackendDto::{Asio, Wasapi};
        use HeadphonesOutputDto::{Auto, Pair};
        let headphones = |backend, outputs, decks, headphones| {
            let mixer = MixerPreferences { mode: MixModeDto::External, headphones, ..Default::default() };
            let info = AudioInfo {
                backend,
                device: String::new(),
                sample_rate: 48_000,
                output_channels: outputs,
                input_channels: 4,
                decks,
                simulated: false,
                missing: None,
            };
            let routing = routing_for(mixer, &info);
            assert_eq!(routing.mode, MixMode::External);
            routing.headphones
        };
        assert_eq!(headphones(Asio, 4, 2, Auto), None, "both pairs go to the mixer");
        assert_eq!(headphones(Asio, 6, 2, Auto), Some(4));
        assert_eq!(headphones(Asio, 8, 4, Auto), None);
        assert_eq!(headphones(Asio, 10, 4, Auto), Some(8));
        assert_eq!(headphones(Wasapi, 6, 2, Auto), None, "maybe a computer's surround");
        assert_eq!(headphones(Wasapi, 6, 2, Pair { first: 4 }), Some(4), "chosen on purpose");
        assert_eq!(headphones(Asio, 6, 2, Pair { first: 2 }), None, "deck B's");
        assert_eq!(headphones(Asio, 4, 4, Pair { first: 2 }), None, "decks B and D's");
    }

    #[test]
    fn the_booth_takes_a_pair_of_its_own() {
        assert_eq!(booth_output(Some(4), Some(2), 6), Some(4));
        assert_eq!(booth_output(Some(2), None, 4), Some(2), "no headphones: 3-4 are free");
        assert_eq!(booth_output(Some(2), Some(2), 4), None, "the headphones are there");
        assert_eq!(booth_output(Some(4), Some(2), 4), None, "not on this device");
        assert_eq!(booth_output(Some(0), None, 4), None, "the master is there");
        assert_eq!(booth_output(None, None, 8), None);
    }

    fn info(decks: usize, input_channels: usize) -> AudioInfo {
        AudioInfo {
            backend: BackendDto::Asio,
            device: String::new(),
            sample_rate: 48_000,
            output_channels: 4,
            input_channels,
            decks,
            simulated: false,
            missing: None,
        }
    }

    #[test]
    fn decks_layered_on_a_turntable_read_the_record_of_the_one_that_has_it() {
        use prisme_timecode::{SERATO_CD, SERATO_CV02_A, SERATO_CV02_B};
        let deck = |timecode, vinyl_enabled| DeckSettings {
            timecode,
            vinyl_enabled,
            ..DeckSettings::from(DeckPreferences::default())
        };
        let saved = [
            deck(&SERATO_CV02_A, false),
            deck(&SERATO_CV02_B, true),
            deck(&SERATO_CD, true),
            deck(&SERATO_CV02_A, true),
        ];
        let ids = |decks: &[DeckSettings; MAX_DECKS]| decks.map(|d| d.timecode.id);

        // C is on the record and A is not: C has turntable A; B has turntable B
        let mut decks = saved;
        assert_eq!(share_records(&mut decks, &info(4, 4)), [0, 3]);
        assert_eq!(ids(&decks), ["serato_cd", "serato_cv02_b", "serato_cd", "serato_cv02_b"]);
        assert!(share_records(&mut decks, &info(4, 4)).is_empty());

        // a turntable each: every deck keeps its own
        let mut decks = saved;
        assert!(share_records(&mut decks, &info(4, 8)).is_empty());
        assert!(share_records(&mut decks, &info(2, 4)).is_empty());
        assert_eq!(ids(&decks), ids(&saved));
    }

    #[test]
    fn the_live_inputs_are_those_the_turntables_leave() {
        assert_eq!(free_inputs(&info(2, 4)), Vec::<usize>::new(), "a turntable on each pair");
        assert_eq!(free_inputs(&info(2, 8)), [4, 5, 6, 7]);
        assert_eq!(free_inputs(&info(4, 8)), Vec::<usize>::new());
        assert_eq!(free_inputs(&info(4, 4)), Vec::<usize>::new(), "C and D layered on A and B");
        assert_eq!(free_inputs(&info(2, 6)), [4, 5]);
        let mono = |first| Some(InputChannelsDto { first, stereo: false });
        let pair = |first| Some(InputChannelsDto { first, stereo: true });
        let free = free_inputs(&info(2, 7));
        assert_eq!(live_channels(mono(6), &free), mono(6));
        assert_eq!(live_channels(pair(4), &free), pair(4));
        assert_eq!(live_channels(pair(6), &free), None, "input 8 is not there");
        assert_eq!(live_channels(mono(1), &free), None, "a turntable is there");
    }

    fn place(load_id: u64, position_s: f64, playing: bool) -> Option<Place> {
        let (looping, sync_locked, leader, slip, reverse) = (None, false, false, false, false);
        Some(Place { load_id, position_s, playing, looping, sync_locked, leader, slip, reverse })
    }

    #[test]
    fn a_restart_puts_the_decks_where_the_last_engine_to_run_had_them() {
        let carried = [place(1, 192.0, true), place(2, 65.0, false), None, None];
        // deck B's track was loaded since
        let loads = [Some(1), Some(5), None, None];
        let kept = [place(1, 192.0, true), None, None, None];
        assert_eq!(places_of(None, loads, carried), kept, "the start before failed");
        let mut status = EngineStatus { deck_count: 2, ..Default::default() };
        assert_eq!(places_of(Some((status, 40)), loads, carried), kept, "its engine has not run");
        status.commands = 40;
        status.decks[0].position_s = 193.0;
        status.decks[0].playing = true;
        assert_eq!(
            places_of(Some((status, 40)), loads, carried),
            [place(1, 193.0, true), place(5, 0.0, false), None, None]
        );
    }

    /// Deck A of an engine that has run `commands`, a track on it.
    fn deck_after(commands: impl IntoIterator<Item = Command>) -> DeckStatus {
        let config = EngineConfig::new(48_000, 2, &prisme_timecode::SERATO_CV02_A);
        let (engine, mut controller) = prisme_engine::create(config);
        let (run, thread) = running(engine);
        let track = Arc::new(TrackAudio::from_interleaved(48_000, vec![0.0; 2 * 48_000 * 60]));
        let loaded = [
            Command::LoadTrack { deck: 0, track },
            Command::SetBeatGrid { deck: 0, grid: Some(BeatGrid { bpm: 120.0, first_beat_s: 0.0 }) },
        ];
        send_all(&mut controller, loaded.into_iter().chain(commands).collect(), RESTORE_WAIT);
        let sent = controller.sent();
        let status = loop {
            let status = controller.status();
            if status.commands >= sent {
                break status;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        run.store(false, Ordering::Relaxed);
        thread.join().unwrap();
        status.decks[0]
    }

    #[test]
    fn a_restart_carries_the_loop_the_sync_the_slip_and_the_reverse_over() {
        let before = deck_after([
            Command::Seek { deck: 0, seconds: 10.0 },
            Command::SetPlaying { deck: 0, playing: true },
            Command::SetSlip { deck: 0, enabled: true },
            Command::BeatLoop { deck: 0, beats: 4.0 },
            Command::SetReverse { deck: 0, enabled: true },
            Command::SetSyncLocked { deck: 0, locked: true },
        ]);
        let place = Place::of(1, &before);
        let after = deck_after(place.commands(0));
        assert_eq!(after.looping.region, Some(LoopRegion { start_s: 10.0, end_s: 12.0 }));
        assert!(after.looping.active && after.playing && after.slip_mode && after.reverse, "{after:?}");
        assert!(after.sync.locked);
        assert!((after.position_s - place.position_s).abs() < 0.1, "{after:?}");
    }

    #[test]
    fn a_restart_keeps_the_leader_chosen_and_the_one_in_effect() {
        let state = AppState::new(JsonFile::in_memory(Default::default()));
        *lock(&state.sync_leader) = Some(1);
        let config = EngineConfig::new(48_000, 2, &prisme_timecode::SERATO_CV02_A);
        // deck B stopped, deck A led in its place
        let leading = place(1, 10.0, true).map(|p| Place { leader: true, ..p });
        let places = [leading, place(2, 20.0, false), None, None];
        let commands = state.restore_commands(&info(2, 4), &config, &places);
        assert!(commands.iter().any(|c| matches!(c, Command::SetSyncLeader { deck: Some(1) })));
        let resumed: Vec<usize> = commands
            .iter()
            .filter_map(|c| match c {
                Command::ResumeSyncLeader { deck } => Some(*deck),
                _ => None,
            })
            .collect();
        assert_eq!(resumed, [0]);
    }

    #[test]
    fn a_roll_held_as_the_audio_restarts_is_let_go() {
        let rolling = deck_after([
            Command::Seek { deck: 0, seconds: 10.0 },
            Command::SetPlaying { deck: 0, playing: true },
            Command::LoopRoll { deck: 0, beats: Some(0.25) },
        ]);
        assert_eq!(Place::of(1, &rolling).looping, None);
    }

    #[test]
    fn a_load_ends_on_its_deck_only_if_none_was_asked_for_there_since() {
        let turns = LoadTurns::default();
        let slow = turns.ask(1);
        let quick = turns.ask(1);
        let other_deck = turns.ask(0);
        assert!(turns.is_last(1, quick) && !turns.is_last(1, slow));
        assert!(turns.is_last(0, other_deck));
    }

    #[test]
    fn a_deck_loads_until_the_last_load_asked_for_there_is_over() {
        let state = AppState::new(JsonFile::in_memory(Default::default()));
        let slow = state.ask_load(1);
        let quick = state.ask_load(1);
        assert_eq!(state.loading(), [false, true, false, false]);
        drop(quick);
        assert_eq!(state.loading(), [false; MAX_DECKS], "the slow one ends nowhere");
        let revision = state.revision();
        drop(slow);
        assert_eq!(state.loading(), [false; MAX_DECKS]);
        assert_eq!(state.moved_since(revision), [Section::Tracks], "the interface hears of it");
    }

    #[test]
    fn a_track_read_or_found_nowhere_moves_no_revision() {
        let bench = crate::actions::bench::Bench::new();
        bench.load(0, 60.0, None);
        let before = bench.state.revision();
        let load_id = bench.state.with_track(0, |t| t.load_id).unwrap();
        assert_eq!(bench.state.with_loaded(0, load_id, |t| t.key_shift), Some(0));
        assert!(bench.state.change_track(1, |t| t.key_shift = 1).is_err());
        assert!(bench.state.change_loaded(0, load_id + 1, |t| t.key_shift = 1).is_none());
        assert!(bench.state.change_file(Path::new("elsewhere.flac"), |t| t.key_shift = 1).is_empty());
        bench.press("deck1.vocals", 1.0).unwrap();
        assert_eq!(bench.state.moved_since(before), [], "a track without stems");
        bench.state.change_loaded(0, load_id, |t| t.key_shift = 1).unwrap();
        assert_eq!(bench.state.moved_since(before), [Section::Tracks]);
    }

    #[test]
    fn the_interface_hears_of_the_audio_stopped_or_started_as_of_what_it_lays_out() {
        let bench = crate::actions::bench::Bench::new();
        let revision = bench.state.revision();
        let session = bench.state.replace_session(None);
        assert!(session.is_some());
        assert_eq!(bench.state.moved_since(revision), [Section::Mixer]);
        bench.state.replace_session(session);
        assert!(lock(&bench.state.session).is_some());
    }

    /// An engine called every few milliseconds, as by a sound card, until the returned flag
    /// falls.
    fn running(mut engine: prisme_engine::Engine) -> (Arc<AtomicBool>, std::thread::JoinHandle<()>) {
        let run = Arc::new(AtomicBool::new(true));
        let runs = run.clone();
        let thread = std::thread::spawn(move || {
            let mut output = [0.0; 2 * 128];
            while runs.load(Ordering::Relaxed) {
                engine.process(&[], 0, &mut output, 2);
                std::thread::sleep(Duration::from_millis(3));
            }
        });
        (run, thread)
    }

    /// More commands than the engine's queue holds.
    const MOVES: usize = 5000;

    fn crossfader_moves(count: usize) -> Vec<Command> {
        (0..count).map(|i| Command::SetCrossfader { position: i as f32 / count as f32 }).collect()
    }

    #[test]
    fn the_simulated_turntable_takes_deck_a_as_it_comes_only() {
        let state = AppState::new(JsonFile::in_memory(Default::default()));
        state.simulate_turntable(false);
        let deck = lock(&state.decks)[0];
        assert!(deck.vinyl_enabled && deck.lead_in_s == Some(10.0));
        // the DJ plays deck A by itself, then the audio starts again
        lock(&state.decks)[0].vinyl_enabled = false;
        state.simulate_turntable(true);
        assert!(!lock(&state.decks)[0].vinyl_enabled);
    }

    #[test]
    fn a_restore_longer_than_the_queue_reaches_the_engine_whole() {
        let config = EngineConfig::new(48_000, 2, &prisme_timecode::SERATO_CV02_A);
        let (engine, mut controller) = prisme_engine::create(config);
        let (run, thread) = running(engine);
        send_all(&mut controller, crossfader_moves(MOVES), RESTORE_WAIT);
        run.store(false, Ordering::Relaxed);
        thread.join().unwrap();
        assert_eq!(controller.sent(), MOVES as u64);
    }

    #[test]
    fn an_engine_that_does_not_run_is_not_waited_for_long() {
        let config = EngineConfig::new(48_000, 2, &prisme_timecode::SERATO_CV02_A);
        let (_engine, mut controller) = prisme_engine::create(config);
        send_all(&mut controller, crossfader_moves(MOVES), Duration::from_millis(20));
        assert!(controller.sent() < MOVES as u64);
    }

    fn db(gain: f32) -> f32 {
        20.0 * gain.log10()
    }

    #[test]
    fn a_loud_track_comes_down_to_the_target() {
        let gain = auto_gain_for(Loudness { lufs: -6.0, peak: 1.0 });
        assert!((db(gain) + 4.0).abs() < 1e-3, "{}", db(gain));
    }

    #[test]
    fn a_quiet_track_goes_up_as_far_as_its_peak_allows() {
        assert!((db(auto_gain_for(Loudness { lufs: -14.0, peak: 0.25 })) - 4.0).abs() < 1e-3);
        let gain = auto_gain_for(Loudness { lufs: -14.0, peak: 0.9 });
        assert!((gain - 1.0 / 0.9).abs() < 1e-5, "{gain}");
        assert_eq!(auto_gain_for(Loudness { lufs: -14.0, peak: 1.0 }), 1.0);
    }

    #[test]
    fn the_correction_is_bounded() {
        assert!((db(auto_gain_for(Loudness { lufs: 5.0, peak: 1.0 })) + 12.0).abs() < 1e-3);
        assert!((db(auto_gain_for(Loudness { lufs: -40.0, peak: 0.01 })) - 6.0).abs() < 1e-3);
    }
}
