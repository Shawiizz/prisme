//! Streams a turntable's signal to the frontend while its oscilloscope is shown.

use crate::audio_host::DECKS;
use crate::error::AppResult;
use crate::state::{lock, AppState};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

const INTERVAL: Duration = Duration::from_millis(33);
/// Points sent per update: the most recent ones, enough to draw the circle.
const POINTS: usize = 1024;

/// Per deck, incremented by every subscription and unsubscription: a stream stops when
/// it sees another generation.
static GENERATIONS: [AtomicU64; DECKS] = [const { AtomicU64::new(0) }; DECKS];

#[tauri::command]
pub fn subscribe_scope(app: AppHandle, deck: usize, channel: Channel<Vec<[f32; 2]>>) -> AppResult<()> {
    AppState::check_deck(deck)?;
    let generation = GENERATIONS[deck].fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::Builder::new().name("prisme-scope".into()).spawn(move || {
        let state = app.state::<AppState>();
        let mut points = Vec::new();
        while GENERATIONS[deck].load(Ordering::SeqCst) == generation {
            points.clear();
            if let Some(session) = lock(&state.session).as_mut() {
                session.controller.read_scope(deck, &mut points);
            }
            let recent = points[points.len().saturating_sub(POINTS)..].to_vec();
            if channel.send(recent).is_err() {
                break;
            }
            std::thread::sleep(INTERVAL);
        }
    })?;
    Ok(())
}

#[tauri::command]
pub fn unsubscribe_scope(deck: usize) -> AppResult<()> {
    AppState::check_deck(deck)?;
    GENERATIONS[deck].fetch_add(1, Ordering::SeqCst);
    Ok(())
}
