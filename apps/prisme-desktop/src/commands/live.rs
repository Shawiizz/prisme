//! The live sources of the software mixer: a microphone and an aux line, and talkover.

use crate::actions::mixer;
use crate::dto::{InputChannelsDto, LiveSourceDto};
use crate::error::AppResult;
use crate::state::{lock, AppState};
use prisme_engine::Command;
use tauri::State;

/// The inputs `source` comes in on, kept for the next start; returns whether they are in use
/// on the running device (the turntables leave them free).
#[tauri::command]
pub fn set_live_input(
    state: State<'_, AppState>,
    source: LiveSourceDto,
    input: Option<InputChannelsDto>,
) -> AppResult<bool> {
    state.update_preferences(|p| match source {
        LiveSourceDto::Mic => p.mixer.mic.input = input,
        LiveSourceDto::Aux => p.mixer.aux.input = input,
    });
    let info = lock(&state.session).as_ref().map(|s| s.info.clone());
    let channels = state.live_input(source, info.as_ref());
    state.send_if_running(Command::SetLiveInput {
        source: source.into(),
        channels: channels.map(Into::into),
    })?;
    Ok(channels.is_some())
}

/// Kept for the next start.
#[tauri::command]
pub fn set_live_level(state: State<'_, AppState>, source: LiveSourceDto, gain: f32) -> AppResult<()> {
    mixer::set_live_level(&state, source, gain)
}

/// Heard in the master, or not.
#[tauri::command]
pub fn set_live_on(state: State<'_, AppState>, source: LiveSourceDto, on: bool) -> AppResult<()> {
    lock(&state.live)[source as usize].on = on;
    state.send_if_running(Command::SetLiveOn { source: source.into(), on })
}

/// Heard in the headphones, or not.
#[tauri::command]
pub fn set_live_cue(state: State<'_, AppState>, source: LiveSourceDto, cue: bool) -> AppResult<()> {
    lock(&state.live)[source as usize].cue = cue;
    state.send_if_running(Command::SetLiveCue { source: source.into(), cue })
}

/// While the microphone is on, the music ducks; kept for the next start.
#[tauri::command]
pub fn set_talkover(state: State<'_, AppState>, enabled: bool) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.talkover = enabled);
    send_talkover(&state)
}

/// How far the music ducks under the microphone (dB); kept for the next start.
#[tauri::command]
pub fn set_talkover_depth(state: State<'_, AppState>, db: f32) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.talkover_depth_db = db.min(0.0));
    send_talkover(&state)
}

fn send_talkover(state: &AppState) -> AppResult<()> {
    let (enabled, depth_db) = state.preferences.read(|p| (p.mixer.talkover, p.mixer.talkover_depth_db));
    state.send_if_running(Command::SetTalkover { enabled, depth_db })
}
