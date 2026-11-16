//! Application state shared by the commands: the preferences, the audio session, the
//! loaded tracks and the deck settings (kept so they can be re-applied when the audio
//! restarts).

use crate::audio_host::{deck_pair, layer_partner, AudioSession};
use crate::dto::{
    AudioInfo, BackendDto, BandDto, ChannelStripDto, CrossfaderSideDto, CueDto, FxChainDto, FxPartDto,
    HeadphonesOutputDto, InputChannelsDto, KeyDto, LiveSourceDto, MixModeDto, RpmDto, SeparationDto,
    StemMixDto, TempoDto, TrackInfoDto, VinylModeDto,
};
use crate::error::{AppError, AppResult};
use crate::folders;
use crate::preferences::{DeckPreferences, LivePreferences, MixerPreferences, Preferences};
use crate::recording::Recorder;
use crate::sampler::Samplers;
use prisme_analysis::{TrackAnalysis, Waveform};
use prisme_engine::{BeatGrid, Command, DeckStatus, MixMode, OutputRouting, StemTracks, MAX_DECKS};
use prisme_library::{Cue, CueSlot, TrackEdits};
use prisme_media::{TrackAudio, TrackTags};
use prisme_session::JsonFile;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use tracing::error;

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
    /// `None` until computed (first load of the track).
    pub waveform: Option<Waveform>,
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
            waveform_ready: self.waveform.is_some(),
            tempo: analysis.tempo.as_ref().map(TempoDto::from),
            key: analysis.key.as_ref().map(KeyDto::from),
            grid_edited: self.edits.grid.is_some(),
            key_edited: self.edits.key.is_some(),
            cues: self.cue_dtos(),
            key_shift: self.key_shift,
            separation: SeparationDto::of(|stem| self.stems[stem as usize].is_some()),
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
    pub key_lock: bool,
    /// Software mixer: the channel fader starts and stops the deck.
    pub fader_start: bool,
    /// The range of its tempo fader, if it has one of its own.
    pub pitch_range: Option<f64>,
    pub strip: ChannelStripDto,
    pub fx: FxChainDto,
    pub sampler_gain: f32,
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
            key_lock: p.key_lock,
            fader_start: p.fader_start,
            pitch_range: p.pitch_range,
            strip: ChannelStripDto::default(),
            fx: FxChainDto::default(),
            sampler_gain: 1.0,
            passthrough: false,
            side: None,
            jog_scratch: true,
            repeat: false,
            mute: false,
        }
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
    pub preferences: JsonFile<Preferences>,
    pub session: Mutex<Option<AudioSession>>,
    pub tracks: Mutex<[Option<LoadedTrack>; MAX_DECKS]>,
    pub decks: Mutex<[DeckSettings; MAX_DECKS]>,
    pub crossfader: Mutex<f32>,
    /// The microphone's and the aux line's, in the order of [`LiveSourceDto::ALL`].
    pub live: Mutex<[LiveSwitches; 2]>,
    pub recorder: Mutex<Option<Recorder>>,
    /// Held while the audio starts: the watchdog and the user never start it at once.
    pub audio_start: Mutex<()>,
    pub samplers: Mutex<Samplers>,
    /// The interface is out of sight: a phone's application in the background, which the
    /// system may stop calling for audio.
    pub hidden: AtomicBool,
}

/// The speed (1: as recorded) from which a track counts as moving: a record at rest
/// trembles under its needle.
const MOVING_SPEED: f64 = 0.05;

/// Locks a mutex, recovering from a poisoned lock (a panicking command must not take
/// the whole application down).
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl AppState {
    pub fn new(preferences: JsonFile<Preferences>) -> Self {
        if preferences.read(|p| p.library.clone().settle()) {
            let _ = preferences.update(|p| p.library.settle());
        }
        let saved = preferences.get().decks;
        Self {
            preferences,
            session: Mutex::new(None),
            tracks: Mutex::default(),
            decks: Mutex::new(saved.map(DeckSettings::from)),
            crossfader: Mutex::new(0.0),
            live: Mutex::default(),
            recorder: Mutex::new(None),
            audio_start: Mutex::new(()),
            samplers: Mutex::default(),
            hidden: AtomicBool::new(false),
        }
    }

    /// Whether `deck` is one the engine can have, kept for when it is in use (its settings);
    /// [`AppState::has_deck`] for one in use.
    pub fn check_deck(deck: usize) -> AppResult<()> {
        if deck < MAX_DECKS {
            Ok(())
        } else {
            Err(AppError::InvalidDeck(deck))
        }
    }

    /// Changes the preferences and saves them; a failed save is reported, not fatal.
    pub fn update_preferences(&self, change: impl FnOnce(&mut Preferences)) {
        if let ((), Err(e)) = self.preferences.update(change) {
            error!("preferences not saved: {e}");
        }
    }

    /// The engine's latest view of `deck`.
    pub fn deck_status(&self, deck: usize) -> AppResult<DeckStatus> {
        Self::check_deck(deck)?;
        let mut session = lock(&self.session);
        let session = session.as_mut().ok_or(AppError::AudioNotRunning)?;
        Ok(session.controller.status().decks[deck])
    }

    /// Position and play state of each deck, to carry them over an audio restart.
    pub fn transport_snapshot(&self) -> Vec<(f64, bool)> {
        let mut session = lock(&self.session);
        let Some(session) = session.as_mut() else { return Vec::new() };
        let status = session.controller.status();
        status.decks[..status.deck_count].iter().map(|d| (d.position_s, d.playing)).collect()
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

    /// Runs `f` on the track on `deck` if it is still the load `load_id`.
    pub fn with_loaded<R>(
        &self,
        deck: usize,
        load_id: u64,
        f: impl FnOnce(&mut LoadedTrack) -> R,
    ) -> Option<R> {
        let mut tracks = lock(&self.tracks);
        tracks.get_mut(deck)?.as_mut().filter(|t| t.load_id == load_id).map(f)
    }

    /// Runs `f` on every loaded track playing `audio` (a load and its instant doubles): what
    /// is found out about a load goes to them all. The deck, load and result of each.
    pub fn with_audio<R>(
        &self,
        audio: &Arc<TrackAudio>,
        f: impl FnMut(&mut LoadedTrack) -> R,
    ) -> Vec<(usize, u64, R)> {
        self.with_tracks(|t| Arc::ptr_eq(&t.audio, audio), f)
    }

    /// Runs `f` on every loaded track of the file `path` (loaded twice, or doubled): what is
    /// changed of a file changes on every deck playing it. The deck, load and result of each.
    pub fn with_path<R>(&self, path: &Path, f: impl FnMut(&mut LoadedTrack) -> R) -> Vec<(usize, u64, R)> {
        self.with_tracks(|t| t.path == path, f)
    }

    fn with_tracks<R>(
        &self,
        which: impl Fn(&LoadedTrack) -> bool,
        mut f: impl FnMut(&mut LoadedTrack) -> R,
    ) -> Vec<(usize, u64, R)> {
        lock(&self.tracks)
            .iter_mut()
            .enumerate()
            .filter_map(|(deck, t)| {
                let t = t.as_mut().filter(|t| which(t))?;
                Some((deck, t.load_id, f(t)))
            })
            .collect()
    }

    /// The decks playing `audio`, and their loads (see [`AppState::with_audio`]).
    pub fn loads_of(&self, audio: &Arc<TrackAudio>) -> Vec<(usize, u64)> {
        self.with_audio(audio, |_| ()).into_iter().map(|(deck, load_id, ())| (deck, load_id)).collect()
    }

    /// Runs `f` on the track loaded on `deck`.
    pub fn with_track<R>(&self, deck: usize, f: impl FnOnce(&mut LoadedTrack) -> R) -> AppResult<R> {
        Self::check_deck(deck)?;
        let mut tracks = lock(&self.tracks);
        tracks[deck].as_mut().map(f).ok_or(AppError::NoTrack(deck))
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
        let _ = self.send_if_running(Command::SetVinylEnabled { deck: partner, enabled: false });
        Some(partner)
    }

    /// The settings of the decks as the device of `info` lets them be: of two decks layered
    /// on a turntable and both asked to follow it, the first does. What was asked stays, for
    /// a device with a turntable each.
    pub fn decks_in_use(&self, info: Option<&AudioInfo>) -> [DeckSettings; MAX_DECKS] {
        let mut decks = *lock(&self.decks);
        for deck in 0..info.map_or(0, |info| info.decks) {
            let first = info
                .and_then(|info| layer_partner(deck, info.decks, info.input_channels))
                .filter(|&partner| partner < deck);
            if first.is_some_and(|first| decks[first].vinyl_enabled) {
                decks[deck].vinyl_enabled = false;
            }
        }
        decks
    }

    /// Whether `deck` is one of the decks in use (two, or four).
    pub fn has_deck(&self, deck: usize) -> AppResult<()> {
        if deck < self.preferences.read(|p| p.dj.decks()) {
            Ok(())
        } else {
            Err(AppError::InvalidDeck(deck))
        }
    }

    /// Re-applies tracks, deck and mixer settings to a freshly started engine.
    pub fn restore(&self, session: &mut AudioSession) {
        let routing = self.routing(&session.info);
        let config = session.engine_config;
        let controller = &mut session.controller;
        let auto_gain = self.preferences.read(|p| p.dj.auto_gain);
        let tracks = lock(&self.tracks);
        let decks = self.decks_in_use(Some(&session.info));
        for (deck, (track, settings)) in tracks.iter().zip(decks.iter()).enumerate() {
            for c in track.iter().flat_map(|t| t.load_commands(deck, auto_gain)) {
                let _ = controller.send(c);
            }
            for c in deck_commands(deck, settings) {
                let _ = controller.send(c);
            }
            if settings.swap_channels {
                let _ = controller.send(Command::ReplaceDecoder { deck, decoder: config.decoder(true) });
            }
        }
        for (deck, slots) in lock(&self.samplers).iter().enumerate() {
            for (slot, loaded) in slots.iter().enumerate() {
                if let Some(loaded) = loaded {
                    let _ = controller.send(Command::SetSample {
                        deck,
                        slot,
                        sample: Some(loaded.audio.clone()),
                    });
                }
            }
        }
        let _ = controller.send(Command::SetCrossfader { position: *lock(&self.crossfader) });
        let preferences = self.preferences.get();
        let _ = controller.send(Command::SetQuantize { enabled: preferences.dj.quantize });
        let _ = controller.send(Command::SetRouting { routing });
        let _ = controller.send(Command::SetCueMix { mix: preferences.mixer.cue_mix });
        let _ = controller.send(Command::SetHeadphoneGain { gain: preferences.mixer.headphone_gain });
        let _ = controller.send(Command::SetSplitCue { enabled: preferences.mixer.split_cue });
        let _ =
            controller.send(Command::SetCrossfaderCurve { curve: preferences.mixer.crossfader_curve.into() });
        let _ = controller.send(Command::SetLimiter { enabled: preferences.mixer.limiter });
        let _ =
            controller.send(Command::SetCrossfaderReverse { enabled: preferences.mixer.crossfader_reverse });
        for c in self.live_commands(Some(&session.info)) {
            let _ = controller.send(c);
        }
    }
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
        }
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
