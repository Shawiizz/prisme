//! A computer: held awake by Prisme itself while a deck plays.

use super::KeepAwake;
use crate::state::AppState;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tracing::error;

/// How long the computer stays awake once every deck stopped: the moment between two
/// tracks, or a record changed.
const LINGER: Duration = Duration::from_secs(60);
const CHECK: Duration = Duration::from_secs(1);

/// Holds the computer awake while a deck plays, and for [`LINGER`] after the last one
/// stopped.
pub fn while_playing(app: AppHandle) {
    let spawned = std::thread::Builder::new().name("prisme-keep-awake".into()).spawn(move || {
        let state = app.state::<AppState>();
        let mut last_played = LastPlayed::default();
        // asked for once a deck plays, granted or not: asked again for the next set only
        let mut request: Option<Option<KeepAwake>> = None;
        loop {
            std::thread::sleep(CHECK);
            if !last_played.keeps_awake(state.is_playing(), Instant::now()) {
                request = None;
            } else if request.is_none() {
                request = Some(KeepAwake::new());
            }
        }
    });
    if let Err(e) = spawned {
        error!("the computer may sleep during a set: {e}");
    }
}

/// When a deck last played.
#[derive(Debug, Default)]
struct LastPlayed(Option<Instant>);

impl LastPlayed {
    /// Whether the computer stays awake at `now`, a deck `playing` or not.
    fn keeps_awake(&mut self, playing: bool, now: Instant) -> bool {
        if playing {
            self.0 = Some(now);
        }
        self.0.is_some_and(|at| now.duration_since(at) < LINGER)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_computer_stays_awake_while_a_deck_plays_and_a_moment_after() {
        let mut last_played = LastPlayed::default();
        let start = Instant::now();
        assert!(!last_played.keeps_awake(false, start), "nothing played yet");
        assert!(last_played.keeps_awake(true, start));
        assert!(last_played.keeps_awake(false, start + LINGER / 2));
        assert!(!last_played.keeps_awake(false, start + LINGER));
        assert!(last_played.keeps_awake(true, start + 2 * LINGER), "the next track");
    }
}
