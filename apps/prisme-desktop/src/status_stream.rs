//! Streams the engine state to the frontend about 60 times per second, and frees what
//! the engine hands back at the same pace.

use crate::dto::StatusDto;
use crate::state::{lock, AppState};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

const INTERVAL: Duration = Duration::from_millis(16);

/// Incremented by every subscription: an older stream stops when it sees a newer one
/// (e.g. after a page reload).
static GENERATION: AtomicU64 = AtomicU64::new(0);

#[tauri::command]
pub fn subscribe_status(app: AppHandle, channel: Channel<StatusDto>) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::Builder::new()
        .name("prisme-status".into())
        .spawn(move || {
            let state = app.state::<AppState>();
            while GENERATION.load(Ordering::SeqCst) == generation {
                let status = {
                    let mut session = lock(&state.session);
                    match session.as_mut() {
                        Some(s) => {
                            s.controller.collect_garbage();
                            StatusDto::from_engine(&s.controller.status(), &s.stats)
                        }
                        None => StatusDto::stopped(),
                    }
                };
                if channel.send(status).is_err() {
                    break; // the window is gone
                }
                std::thread::sleep(INTERVAL);
            }
        })
        .expect("cannot start the status thread");
}
