//! Keeps the audio going, with the decks where they were: restarts it when its device
//! comes back after a loss, when the engine failed, or when the driver stopped calling it,
//! and moves it to the saved devices once they appear (the audio started on the default
//! ones because the interface was off).

use crate::audio_host::devices_available;
use crate::commands::audio::start_audio;
use crate::dto::AudioSettings;
use crate::state::{lock, AppState};
use prisme_audio_io::system_devices;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tracing::{error, info, warn};

const CHECK: Duration = Duration::from_millis(500);
const RETRY: Duration = Duration::from_secs(2);
/// A lost device is tried again at once, then this often for a few times (a Bluetooth
/// headset or an interface back within a second), then every `RETRY`.
const QUICK_RETRY: Duration = Duration::from_millis(200);
const QUICK_RETRIES: usize = 5;
/// More engine failures than this within the window are not restarted: the failure
/// would only come back.
const MAX_RESTARTS: usize = 3;
const RESTART_WINDOW: Duration = Duration::from_secs(60);
/// Looks for the saved devices after the system's devices changed.
const LOOKS_AFTER_A_CHANGE: u32 = 5;
/// Sent when the audio moved to other devices by itself: the interface shows them again.
pub const AUDIO_CHANGED_EVENT: &str = "audio-changed";

pub fn start(app: AppHandle) {
    std::thread::Builder::new()
        .name("prisme-audio-watchdog".into())
        .spawn(move || {
            let state = app.state::<AppState>();
            let mut restarts = Restarts::default();
            let mut last_periods = None;
            let mut last_look = Instant::now();
            let mut devices: Option<Vec<String>> = None;
            let mut looks_left = 0;
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
                    let _ = app.emit(AUDIO_CHANGED_EVENT, ());
                    last_periods = None;
                    continue;
                }
                if last_look.elapsed() >= RETRY {
                    last_look = Instant::now();
                    let saved =
                        AudioSettings { simulate: settings.simulate, ..state.preferences.get().audio };
                    if saved == settings {
                        devices = None;
                    } else if move_to_saved(&state, &app, &saved, &settings, &mut devices, &mut looks_left) {
                        last_periods = None;
                        continue;
                    }
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

/// While the audio runs on other devices than the saved ones, looks for these whenever
/// the system's devices change (loading the drivers to look is slow, and they must not be
/// loaded over and over), a few times as an interface takes a moment to come up. True
/// once the audio has moved.
fn move_to_saved(
    state: &AppState,
    app: &AppHandle,
    saved: &AudioSettings,
    current: &AudioSettings,
    devices: &mut Option<Vec<String>>,
    looks_left: &mut u32,
) -> bool {
    let now = system_devices();
    if devices.as_ref() != Some(&now) {
        *devices = Some(now);
        *looks_left = LOOKS_AFTER_A_CHANGE;
    }
    if *looks_left == 0 {
        return false;
    }
    *looks_left -= 1;
    if !devices_available(saved) {
        return false;
    }
    *looks_left = 0;
    info!("saved audio devices found, moving to them");
    match start_audio(state, saved) {
        Ok(_) => {
            let _ = app.emit(AUDIO_CHANGED_EVENT, ());
        }
        Err(e) => {
            warn!("saved audio devices unusable ({e}), back to the previous ones");
            if let Err(e) = start_audio(state, current) {
                error!("audio could not restart: {e}");
            }
        }
    }
    true
}

fn recover(state: &AppState, settings: &AudioSettings) {
    let waits = std::iter::once(Duration::ZERO)
        .chain(std::iter::repeat_n(QUICK_RETRY, QUICK_RETRIES))
        .chain(std::iter::repeat(RETRY));
    for wait in waits {
        std::thread::sleep(wait);
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
