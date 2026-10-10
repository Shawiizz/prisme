//! History of the sets. A track counts as played once it has been heard for
//! [`PLAYED_AFTER_S`] on a deck: shorter plays are previews or mistakes. The first play
//! of a run of the app opens a new history session.

use crate::audio_host::DECKS;
use crate::library_service::LibraryService;
use crate::state::{lock, AppState};
use prisme_library::SessionId;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

pub const PLAYED_AFTER_S: f64 = 30.0;
pub const HISTORY_CHANGED_EVENT: &str = "history-changed";
const POLL: Duration = Duration::from_millis(250);
/// Slower than this, a deck is stopped or being cued rather than played.
const AUDIBLE_SPEED: f64 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Play {
    pub deck: usize,
    /// When the track started being heard.
    pub started_at_ms: i64,
}

#[derive(Default)]
struct DeckPlay {
    load_id: Option<u64>,
    heard_s: f64,
    started_at_ms: Option<i64>,
    counted: bool,
}

/// Counts how long each loaded track is heard.
#[derive(Default)]
pub struct PlayTracker {
    decks: [DeckPlay; DECKS],
}

impl PlayTracker {
    /// `load_id` identifies the track on the deck (a track loaded again is a new play);
    /// returns the play at the moment the track has been heard long enough.
    pub fn observe(
        &mut self,
        deck: usize,
        load_id: Option<u64>,
        audible: bool,
        dt_s: f64,
        now_ms: i64,
    ) -> Option<Play> {
        let d = &mut self.decks[deck];
        if d.load_id != load_id {
            *d = DeckPlay { load_id, ..DeckPlay::default() };
        }
        if load_id.is_none() || !audible || d.counted {
            return None;
        }
        let started_at_ms = *d.started_at_ms.get_or_insert(now_ms);
        d.heard_s += dt_s;
        d.counted = d.heard_s >= PLAYED_AFTER_S;
        d.counted.then_some(Play { deck, started_at_ms })
    }
}

/// Watches the decks for the whole life of the app.
pub fn start(app: AppHandle) {
    std::thread::Builder::new()
        .name("prisme-history".into())
        .spawn(move || {
            let mut tracker = PlayTracker::default();
            let mut session = None;
            let mut last = Instant::now();
            loop {
                std::thread::sleep(POLL);
                let dt_s = last.elapsed().as_secs_f64();
                last = Instant::now();
                let now_ms = prisme_library::now_ms();
                for (deck, load_id, path, audible) in deck_states(&app.state::<AppState>()) {
                    if let Some(play) = tracker.observe(deck, load_id, audible, dt_s, now_ms) {
                        record(&app, &mut session, &path, play);
                    }
                }
            }
        })
        .expect("cannot start the history thread");
}

fn deck_states(state: &AppState) -> Vec<(usize, Option<u64>, PathBuf, bool)> {
    let speeds: Vec<f64> = {
        let mut session = lock(&state.session);
        let Some(session) = session.as_mut() else { return Vec::new() };
        let status = session.controller.status();
        status.decks.iter().take(DECKS).map(|d| if d.loaded { d.speed } else { 0.0 }).collect()
    };
    let tracks = lock(&state.tracks);
    tracks
        .iter()
        .zip(speeds)
        .enumerate()
        .map(|(deck, (track, speed))| {
            let path = track.as_ref().map(|t| t.path.clone()).unwrap_or_default();
            (deck, track.as_ref().map(|t| t.load_id), path, speed.abs() >= AUDIBLE_SPEED)
        })
        .collect()
}

fn record(app: &AppHandle, session: &mut Option<SessionId>, path: &std::path::Path, play: Play) {
    let library = app.state::<Arc<LibraryService>>();
    let result = match *session {
        Some(id) => Ok(id),
        None => library.db.start_history_session(play.started_at_ms),
    }
    .and_then(|id| {
        *session = Some(id);
        library.db.record_play(id, path, play.deck, play.started_at_ms)
    });
    match result {
        Ok(()) => {
            let _ = app.emit(HISTORY_CHANGED_EVENT, ());
        }
        Err(e) => eprintln!("play not recorded: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds `seconds` of a deck state in 0.25 s steps from `start_ms`; returns the plays.
    fn run(t: &mut PlayTracker, load: Option<u64>, audible: bool, seconds: f64, start_ms: i64) -> Vec<Play> {
        let steps = (seconds / 0.25) as i64;
        (0..steps).filter_map(|i| t.observe(0, load, audible, 0.25, start_ms + i * 250)).collect()
    }

    #[test]
    fn a_track_is_played_once_heard_long_enough() {
        let mut t = PlayTracker::default();
        assert!(run(&mut t, Some(1), true, PLAYED_AFTER_S - 1.0, 10_000).is_empty());
        assert!(run(&mut t, Some(1), false, 60.0, 50_000).is_empty(), "paused time does not count");
        let plays = run(&mut t, Some(1), true, 5.0, 200_000);
        assert_eq!(plays, [Play { deck: 0, started_at_ms: 10_000 }]);
        assert!(run(&mut t, Some(1), true, 120.0, 300_000).is_empty(), "counted once");
    }

    #[test]
    fn previews_do_not_count_and_a_reload_is_a_new_play() {
        let mut t = PlayTracker::default();
        assert!(run(&mut t, Some(1), true, 10.0, 0).is_empty());
        assert!(run(&mut t, Some(2), true, 10.0, 20_000).is_empty(), "the preview time is not carried over");
        assert!(run(&mut t, None, true, 60.0, 40_000).is_empty());
        assert_eq!(run(&mut t, Some(3), true, 40.0, 100_000).len(), 1);
        assert_eq!(run(&mut t, Some(4), true, 40.0, 200_000).len(), 1);
    }
}
