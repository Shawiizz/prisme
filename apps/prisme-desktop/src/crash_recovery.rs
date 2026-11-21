//! Puts the decks back after a crash. While the app runs, what each deck holds and
//! where is saved every few seconds; at the next start, if the app did not close
//! normally, those tracks are loaded again there, stopped.

use crate::library_service::LibraryService;
use crate::state::{lock, AppState};
use crate::track_loading;
use prisme_engine::Command;
use prisme_session::JsonFile;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tracing::warn;

const SAVE_EVERY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LastSession {
    /// Still set at the next start: the app did not close normally.
    pub running: bool,
    pub decks: Vec<Option<SavedDeck>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedDeck {
    pub path: PathBuf,
    pub position_s: f64,
}

pub struct CrashRecovery {
    file: JsonFile<LastSession>,
    closing: AtomicBool,
}

impl CrashRecovery {
    pub fn new(file: JsonFile<LastSession>) -> Self {
        Self { file, closing: AtomicBool::new(false) }
    }

    /// The decks of the previous run if it crashed (empty otherwise).
    pub fn decks_to_restore(&self) -> Vec<Option<SavedDeck>> {
        self.file.read(|s| if s.running { s.decks.clone() } else { Vec::new() })
    }

    /// Loads the saved tracks on the decks still empty, before the audio starts.
    pub fn preload(app: &AppHandle, state: &AppState, library: &LibraryService, decks: &[Option<SavedDeck>]) {
        for (deck, saved) in decks.iter().enumerate() {
            let Some(saved) = saved else { continue };
            if lock(&state.tracks).get(deck).is_none_or(Option::is_some) {
                continue;
            }
            if let Err(e) = track_loading::preload(app, state, library, deck, saved.path.clone()) {
                warn!("{} not restored: {e}", saved.path.display());
            }
        }
    }

    /// Puts the restored decks where they were (once the audio runs), but for one the DJ loaded
    /// another track on meanwhile.
    pub fn seek(state: &AppState, decks: &[Option<SavedDeck>]) {
        let loaded = lock(&state.tracks).each_ref().map(|t| t.as_ref().map(|t| t.path.clone()));
        for (deck, saved) in decks.iter().enumerate() {
            let Some(saved) = saved else { continue };
            if loaded.get(deck).is_some_and(|path| path.as_ref() == Some(&saved.path)) {
                let _ = state.send(Command::Seek { deck, seconds: saved.position_s });
            }
        }
    }

    /// Saves the decks every few seconds while the app runs.
    pub fn start(app: AppHandle) {
        std::thread::Builder::new()
            .name("prisme-crash-recovery".into())
            .spawn(move || {
                let (recovery, state) = (app.state::<CrashRecovery>(), app.state::<AppState>());
                loop {
                    recovery.save(&state);
                    std::thread::sleep(SAVE_EVERY);
                }
            })
            .expect("cannot start the crash recovery thread");
    }

    pub fn closed_normally(&self) {
        self.closing.store(true, Ordering::SeqCst);
        self.file.update(|s| s.running = false);
        self.file.flush();
    }

    fn save(&self, state: &AppState) {
        let places = state.places();
        let decks: Vec<_> = lock(&state.tracks)
            .iter()
            .enumerate()
            .map(|(deck, track)| {
                let track = track.as_ref()?;
                let position_s = places[deck].map_or(0.0, |place| place.position_s);
                Some(SavedDeck { path: track.path.clone(), position_s })
            })
            .collect();
        let now = LastSession { running: true, decks };
        if self.file.read(|s| *s == now) {
            return;
        }
        // checked under the file's lock: no save may follow `closed_normally`
        self.file.update(|s| {
            if !self.closing.load(Ordering::SeqCst) {
                *s = now;
            }
        });
    }
}
