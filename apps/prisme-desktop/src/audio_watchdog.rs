//! Restarts the audio when its device goes away (unplugged), with the decks where they
//! were, as soon as the device is back.

use crate::commands::audio::start_audio;
use crate::state::{lock, AppState};
use std::time::Duration;
use tauri::{AppHandle, Manager};

const CHECK: Duration = Duration::from_millis(500);
const RETRY: Duration = Duration::from_secs(2);

pub fn start(app: AppHandle) {
    std::thread::Builder::new()
        .name("prisme-audio-watchdog".into())
        .spawn(move || {
            let state = app.state::<AppState>();
            loop {
                std::thread::sleep(CHECK);
                if lock(&state.session).as_ref().is_some_and(|s| s.device_lost()) {
                    eprintln!("audio device lost, waiting for it to come back");
                    recover(&state);
                }
            }
        })
        .expect("cannot start the audio watchdog");
}

fn recover(state: &AppState) {
    loop {
        std::thread::sleep(RETRY);
        match start_audio(state, &state.preferences.get().audio) {
            Ok(_) => {
                eprintln!("audio device back");
                return;
            }
            Err(e) => eprintln!("audio still unavailable: {e}"),
        }
    }
}
