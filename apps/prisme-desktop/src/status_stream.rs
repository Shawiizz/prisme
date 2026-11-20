//! Streams the engine state to the frontend about 60 times per second, with what the interface
//! mirrors whenever it changed, and frees what the engine hands back at the same pace.

use crate::dto::{ControlStateDto, StatusDto};
use crate::preview::Previews;
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
            // the revision of what the interface mirrors it was last sent
            let mut told = None;
            while GENERATION.load(Ordering::SeqCst) == generation {
                // read before what it is of: a change after moves it again
                let revision = state.revision();
                let (engine, garbage) = {
                    let mut session = lock(&state.session);
                    session.as_mut().map_or((None, Vec::new()), |s| {
                        let engine = (s.controller.status(), s.stats.clone(), s.engine_config.sample_rate);
                        (Some(engine), s.controller.take_garbage())
                    })
                };
                // freed, and the status built, with the session's lock released: the jogs and
                // the clock wait on it
                drop(garbage);
                if let Some(previews) = app.try_state::<Previews>() {
                    let playing = engine.as_ref().and_then(|(e, ..)| e.preview.as_ref()).map(|p| p.id);
                    previews.follow(&state, playing);
                }
                let mut status = match engine {
                    Some((engine, stats, sample_rate)) => {
                        StatusDto::from_engine(&engine, &stats, sample_rate)
                    }
                    None => StatusDto::stopped(),
                };
                if told != Some(revision) {
                    status.controls = Some(ControlStateDto::of(&state));
                    told = Some(revision);
                }
                if channel.send(status).is_err() {
                    break; // the window is gone
                }
                std::thread::sleep(INTERVAL);
            }
        })
        .expect("cannot start the status thread");
}
