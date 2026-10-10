//! Application state shared by the commands: the audio session, the loaded tracks and
//! the deck settings (kept so they can be re-applied when the audio restarts).

use crate::audio_host::{AudioSession, DECKS};
use crate::dto::{AudioSettings, KeyDto, TempoDto, TrackInfoDto, VinylModeDto, WaveformDto};
use crate::error::{AppError, AppResult};
use crate::library;
use prisme_analysis::{TrackAnalysis, Waveform};
use prisme_engine::{Command, EngineController};
use prisme_media::TrackAudio;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

pub struct LoadedTrack {
    pub path: PathBuf,
    pub audio: Arc<TrackAudio>,
    pub waveform: Waveform,
    pub analysis: TrackAnalysis,
}

impl LoadedTrack {
    /// What the UI shows of the track on `deck`.
    pub fn info(&self, deck: usize) -> TrackInfoDto {
        TrackInfoDto {
            deck,
            path: self.path.display().to_string(),
            title: library::title_from_path(&self.path),
            artist: None,
            duration_s: self.audio.duration_s(),
            sample_rate: self.audio.sample_rate(),
            waveform: WaveformDto::from(&self.waveform),
            tempo: self.analysis.tempo.as_ref().map(TempoDto::from),
            key: self.analysis.key.as_ref().map(KeyDto::from),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DeckSettings {
    pub pitch: f64,
    pub volume: f32,
    pub vinyl_enabled: bool,
    pub vinyl_mode: VinylModeDto,
    pub lead_in_s: f64,
}

impl Default for DeckSettings {
    fn default() -> Self {
        Self { pitch: 1.0, volume: 1.0, vinyl_enabled: false, vinyl_mode: VinylModeDto::Absolute, lead_in_s: 0.0 }
    }
}

#[derive(Default)]
pub struct AppState {
    pub session: Mutex<Option<AudioSession>>,
    pub settings: Mutex<AudioSettings>,
    pub tracks: Mutex<[Option<LoadedTrack>; DECKS]>,
    pub decks: Mutex<[DeckSettings; DECKS]>,
    pub crossfader: Mutex<f32>,
}

/// Locks a mutex, recovering from a poisoned lock (a panicking command must not take
/// the whole application down).
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl AppState {
    pub fn check_deck(deck: usize) -> AppResult<()> {
        if deck < DECKS {
            Ok(())
        } else {
            Err(AppError::InvalidDeck(deck))
        }
    }

    /// Sends a command to the running engine.
    pub fn send(&self, command: Command) -> AppResult<()> {
        let mut session = lock(&self.session);
        let session = session.as_mut().ok_or(AppError::AudioNotRunning)?;
        session.controller.send(command).map_err(|_| AppError::EngineBusy)
    }

    /// Re-applies tracks and deck settings to a freshly started engine.
    pub fn restore(&self, controller: &mut EngineController) {
        let tracks = lock(&self.tracks);
        let decks = lock(&self.decks);
        for (deck, (track, settings)) in tracks.iter().zip(decks.iter()).enumerate() {
            if let Some(t) = track {
                let _ = controller.send(Command::LoadTrack { deck, track: t.audio.clone() });
            }
            for c in deck_commands(deck, settings) {
                let _ = controller.send(c);
            }
        }
        let _ = controller.send(Command::SetCrossfader { position: *lock(&self.crossfader) });
    }
}

/// Commands that put a deck in the state described by `s`.
pub fn deck_commands(deck: usize, s: &DeckSettings) -> [Command; 5] {
    [
        Command::SetPitch { deck, ratio: s.pitch },
        Command::SetVolume { deck, gain: s.volume },
        Command::SetVinylMode { deck, mode: s.vinyl_mode.into() },
        Command::SetLeadIn { deck, seconds: s.lead_in_s },
        Command::SetVinylEnabled { deck, enabled: s.vinyl_enabled },
    ]
}
