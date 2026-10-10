//! Application state shared by the commands: the preferences, the audio session, the
//! loaded tracks and the deck settings (kept so they can be re-applied when the audio
//! restarts).

use crate::audio_host::{AudioSession, DECKS};
use crate::dto::{
    BandDto, ChannelStripDto, CueDto, FxDto, KeyDto, MixModeDto, TempoDto, TrackInfoDto, VinylModeDto,
};
use crate::error::{AppError, AppResult};
use crate::folders;
use crate::preferences::{DeckPreferences, Preferences};
use crate::recording::Recorder;
use crate::sampler::Samplers;
use prisme_analysis::{TrackAnalysis, Waveform};
use prisme_engine::{BeatGrid, Command, DeckStatus, MixMode, OutputRouting};
use prisme_library::{Cue, CueSlot, TrackEdits};
use prisme_media::{TrackAudio, TrackTags};
use prisme_session::JsonFile;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use tracing::error;

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
}

impl LoadedTrack {
    pub fn next_load_id() -> u64 {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        NEXT.fetch_add(1, Ordering::Relaxed)
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
        }
    }

    pub fn analysis(&self) -> TrackAnalysis {
        self.edits.apply(self.detected)
    }

    pub fn grid(&self) -> Option<BeatGrid> {
        self.analysis().tempo.map(|t| BeatGrid { bpm: t.bpm, first_beat_s: t.first_beat_s })
    }

    /// Commands that put the track, with its beat grid and key shift, on `deck`.
    pub fn load_commands(&self, deck: usize) -> [Command; 3] {
        [
            Command::LoadTrack { deck, track: self.audio.clone() },
            Command::SetBeatGrid { deck, grid: self.grid() },
            Command::SetKeyShift { deck, semitones: self.key_shift },
        ]
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
    pub lead_in_s: f64,
    pub swap_channels: bool,
    pub key_lock: bool,
    pub strip: ChannelStripDto,
    pub fx: FxDto,
    pub sampler_gain: f32,
}

impl From<DeckPreferences> for DeckSettings {
    fn from(p: DeckPreferences) -> Self {
        Self {
            pitch: 1.0,
            volume: 1.0,
            cue: false,
            vinyl_enabled: p.vinyl_enabled,
            vinyl_mode: p.vinyl_mode,
            lead_in_s: p.lead_in_s,
            swap_channels: p.swap_channels,
            key_lock: p.key_lock,
            strip: ChannelStripDto::default(),
            fx: FxDto::default(),
            sampler_gain: 1.0,
        }
    }
}

pub struct AppState {
    pub preferences: JsonFile<Preferences>,
    pub session: Mutex<Option<AudioSession>>,
    pub tracks: Mutex<[Option<LoadedTrack>; DECKS]>,
    pub decks: Mutex<[DeckSettings; DECKS]>,
    pub crossfader: Mutex<f32>,
    pub recorder: Mutex<Option<Recorder>>,
    pub samplers: Mutex<Samplers>,
}

/// Locks a mutex, recovering from a poisoned lock (a panicking command must not take
/// the whole application down).
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl AppState {
    pub fn new(preferences: JsonFile<Preferences>) -> Self {
        let saved = preferences.get().decks;
        Self {
            preferences,
            session: Mutex::new(None),
            tracks: Mutex::default(),
            decks: Mutex::new(saved.map(DeckSettings::from)),
            crossfader: Mutex::new(0.0),
            recorder: Mutex::new(None),
            samplers: Mutex::default(),
        }
    }

    pub fn check_deck(deck: usize) -> AppResult<()> {
        if deck < DECKS {
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

    /// Sample rate of the running audio.
    pub fn sample_rate(&self) -> Option<u32> {
        lock(&self.session).as_ref().map(|s| s.engine_config.sample_rate)
    }

    /// The routing for a device with `output_channels`: an external mixer needs an output
    /// pair per deck, else the software mixer is used.
    pub fn routing(&self, output_channels: usize) -> OutputRouting {
        let external =
            self.preferences.get().mixer.mode == MixModeDto::External && output_channels >= 2 * DECKS;
        let mode = if external { MixMode::External } else { MixMode::Internal };
        OutputRouting { mode, ..OutputRouting::default() }
    }

    /// Re-applies tracks, deck and mixer settings to a freshly started engine.
    pub fn restore(&self, session: &mut AudioSession) {
        let output_channels = session.info.output_channels;
        let config = session.engine_config;
        let controller = &mut session.controller;
        let tracks = lock(&self.tracks);
        let decks = lock(&self.decks);
        for (deck, (track, settings)) in tracks.iter().zip(decks.iter()).enumerate() {
            for c in track.iter().flat_map(|t| t.load_commands(deck)) {
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
        let _ = controller.send(Command::SetRouting { routing: self.routing(output_channels) });
        let _ = controller.send(Command::SetCueMix { mix: preferences.mixer.cue_mix });
        let _ = controller.send(Command::SetHeadphoneGain { gain: preferences.mixer.headphone_gain });
        let _ =
            controller.send(Command::SetCrossfaderCurve { curve: preferences.mixer.crossfader_curve.into() });
        let _ = controller.send(Command::SetLimiter { enabled: preferences.mixer.limiter });
    }
}

/// Commands that put a deck in the state described by `s`.
pub fn deck_commands(deck: usize, s: &DeckSettings) -> Vec<Command> {
    let strip = s.strip;
    let mut commands = vec![
        Command::SetDeckCue { deck, enabled: s.cue },
        Command::SetPitch { deck, ratio: s.pitch },
        Command::SetVolume { deck, gain: s.volume },
        Command::SetVinylMode { deck, mode: s.vinyl_mode.into() },
        Command::SetLeadIn { deck, seconds: s.lead_in_s },
        Command::SetVinylEnabled { deck, enabled: s.vinyl_enabled },
        Command::SetKeyLock { deck, enabled: s.key_lock },
        Command::SetTrim { deck, gain: strip.trim },
        Command::SetFilter { deck, position: strip.filter },
        Command::SetFx { deck, kind: s.fx.kind.into() },
        Command::SetFxMix { deck, mix: s.fx.mix },
        Command::SetFxAmount { deck, amount: s.fx.amount },
        Command::SetFxBeats { deck, beats: s.fx.beats },
        Command::SetFxEnabled { deck, enabled: s.fx.enabled },
        Command::SetSamplerGain { deck, gain: s.sampler_gain },
    ];
    for (i, band) in BandDto::ALL.into_iter().enumerate() {
        commands.push(Command::SetEq { deck, band: band.into(), gain: strip.eq[i] });
        commands.push(Command::SetEqKill { deck, band: band.into(), kill: strip.kills[i] });
    }
    commands
}
