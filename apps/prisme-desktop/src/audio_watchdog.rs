//! Keeps the audio going, with the decks where they were: restarts it when its device
//! comes back after a loss, when the engine failed, or when the driver stopped calling it.

use crate::commands::audio::start_audio;
use crate::dto::AudioSettings;
use crate::state::{lock, AppState};
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tracing::{error, info, warn};

const CHECK: Duration = Duration::from_millis(500);
const RETRY: Duration = Duration::from_secs(2);
/// More engine failures than this within the window are not restarted: the failure
/// would only come back.
const MAX_RESTARTS: usize = 3;
const RESTART_WINDOW: Duration = Duration::from_secs(60);

pub fn start(app: AppHandle) {
    std::thread::Builder::new()
        .name("prisme-audio-watchdog".into())
        .spawn(move || {
            let state = app.state::<AppState>();
            let mut restarts = Restarts::default();
            let mut last_periods = None;
            loop {
                std::thread::sleep(CHECK);
                let session = lock(&state.session)
                    .as_ref()
                    .map(|s| (s.device_lost(), s.engine_failed(), s.periods(), s.settings.clone()));
                let Some((lost, failed, periods, settings)) = session else {
                    last_periods = None;
                    continue;
                };
                if lost {
                    info!("audio device lost, waiting for it to come back");
                    recover(&state, &settings);
                    last_periods = None;
                    continue;
                }
                let stalled = last_periods == Some(periods);
                last_periods = Some(periods);
                if (failed || stalled) && restarts.allow(Instant::now()) {
                    warn!("audio {}, restarting it", if failed { "engine failed" } else { "stalled" });
                    if let Err(e) = start_audio(&state, &settings) {
                        error!("audio could not restart: {e}");
                    }
                    last_periods = None;
                }
            }
        })
        .expect("cannot start the audio watchdog");
}

fn recover(state: &AppState, settings: &AudioSettings) {
    loop {
        std::thread::sleep(RETRY);
        match start_audio(state, settings) {
            Ok(_) => {
                info!("audio device back");
                return;
            }
            Err(e) => info!("audio still unavailable: {e}"),
        }
    }
}

/// Recent restarts, to stop restarting a failure that keeps coming back.
#[derive(Default)]
struct Restarts(VecDeque<Instant>);

impl Restarts {
    fn allow(&mut self, now: Instant) -> bool {
        while self.0.front().is_some_and(|&t| now.duration_since(t) > RESTART_WINDOW) {
            self.0.pop_front();
        }
        let allowed = self.0.len() < MAX_RESTARTS;
        if allowed {
            self.0.push_back(now);
        }
        allowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restarts_are_limited_within_the_window() {
        let mut restarts = Restarts::default();
        let start = Instant::now();
        assert!((0..MAX_RESTARTS).all(|i| restarts.allow(start + Duration::from_secs(i as u64))));
        assert!(!restarts.allow(start + Duration::from_secs(30)));
        assert!(restarts.allow(start + RESTART_WINDOW + Duration::from_secs(1)));
    }
}
