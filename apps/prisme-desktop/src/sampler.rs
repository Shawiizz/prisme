//! The decks' samplers: sounds decoded once, kept to be given back to the engine after
//! an audio restart, their files remembered for the next start.

use crate::audio_host::DECKS;
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState};
use prisme_engine::{Command, SAMPLE_SLOTS};
use prisme_media::TrackAudio;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tracing::warn;

/// A sampler is for short sounds; longer files belong on a deck.
pub const MAX_SAMPLE_S: f64 = 30.0;

pub struct LoadedSample {
    pub path: PathBuf,
    pub audio: Arc<TrackAudio>,
}

pub type Samplers = [[Option<LoadedSample>; SAMPLE_SLOTS]; DECKS];

pub fn decode(path: &Path) -> AppResult<Arc<TrackAudio>> {
    let audio = prisme_media::decode_file(path)?;
    if audio.duration_s() > MAX_SAMPLE_S {
        return Err(AppError::SampleTooLong(MAX_SAMPLE_S));
    }
    Ok(Arc::new(audio))
}

/// What the pad shows: the file name without its extension.
pub fn name(path: &Path) -> String {
    path.file_stem().map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into_owned())
}

/// Puts a decoded sound in a slot (and in the engine if it runs).
pub fn put(
    state: &AppState,
    deck: usize,
    slot: usize,
    path: PathBuf,
    audio: Arc<TrackAudio>,
) -> AppResult<()> {
    let sample = Some(audio.clone());
    lock(&state.samplers)[deck][slot] = Some(LoadedSample { path, audio });
    state.send_if_running(Command::SetSample { deck, slot, sample })
}

/// Decodes the sounds saved in the preferences, off the main thread.
pub fn load_saved(app: AppHandle) {
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let saved = state.preferences.get().decks;
        for (deck, preferences) in saved.iter().enumerate() {
            for (slot, path) in preferences.samples.iter().enumerate().take(SAMPLE_SLOTS) {
                let Some(path) = path.as_ref().map(PathBuf::from) else { continue };
                let loaded = decode(&path).and_then(|audio| put(&state, deck, slot, path.clone(), audio));
                if let Err(e) = loaded {
                    warn!("sample {} not loaded: {e}", path.display());
                }
            }
        }
    });
}
