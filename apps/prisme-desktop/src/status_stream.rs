//! Streams the engine state to the frontend about 60 times per second, with the sections of
//! what the interface mirrors that changed since last sent, and frees what the engine hands back
//! at the same pace.

use crate::actions::Actions;
use crate::dto::{ControlStateDto, StatusDto};
use crate::preview::Previews;
use crate::state::{lock, AppState, Section};
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
            let actions = app.state::<Actions>();
            // the revisions of what the interface mirrors it was last sent: none, all sent first
            let mut told = None;
            while GENERATION.load(Ordering::SeqCst) == generation {
                // read before what it is of: a change after moves it again
                let revision = state.revision();
                let (engine, garbage) = {
                    let mut session = lock(&state.session);
                    session.as_mut().map_or((None, Vec::new()), |s| {
                        let stream = (s.stats.clone(), s.engine_config.sample_rate);
                        let engine = (s.controller.status(), s.info.layout(), stream);
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
                    Some((engine, layout, (stats, sample_rate))) => {
                        let view = actions.view_with(&app, engine, Some(layout));
                        StatusDto::of(&view, Some((&stats, sample_rate)))
                    }
                    None => StatusDto::of(&actions.view_with(&app, Default::default(), None), None),
                };
                if told != Some(revision) {
                    let moved =
                        |s: &Section| told.is_none_or(|t: [u64; _]| t[*s as usize] != revision[*s as usize]);
                    status.controls =
                        Some(ControlStateDto::of(&state, Section::ALL.into_iter().filter(moved)));
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
