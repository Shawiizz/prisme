//! Development helpers driven by environment variables, so that the application can be
//! started in a known state without clicking:
//!
//! * `PRISME_DECK1` / `PRISME_DECK2`: audio file to load on the deck at startup;
//! * `PRISME_SIMULATE=1`: start with the simulated turntable on deck 1;
//! * `PRISME_MASTER_GAIN`: initial master gain (e.g. 0.2 to test quietly).

use crate::audio_host::DECKS;
use crate::commands::prepare_track;
use crate::state::{lock, AppState};
use std::path::PathBuf;

/// Settings override requested by the environment.
pub fn simulate_requested() -> bool {
    std::env::var("PRISME_SIMULATE").is_ok_and(|v| v == "1")
}

/// Loads the tracks requested by the environment (before the audio starts, so that
/// `AppState::restore` hands them to the engine).
pub fn preload_tracks(state: &AppState) {
    for deck in 0..DECKS {
        let Ok(path) = std::env::var(format!("PRISME_DECK{}", deck + 1)) else { continue };
        match prepare_track(PathBuf::from(path)) {
            Ok(track) => lock(&state.tracks)[deck] = Some(track),
            Err(e) => eprintln!("PRISME_DECK{}: {e}", deck + 1),
        }
    }
}

/// Master gain requested by the environment.
pub fn master_gain() -> Option<f32> {
    std::env::var("PRISME_MASTER_GAIN").ok().and_then(|v| v.parse().ok())
}
