//! The live sources of the software mixer: a microphone and an aux line, and talkover.

use crate::actions::mixer;
use crate::dto::{InputChannelsDto, LiveSourceDto};
use crate::error::AppResult;
use crate::state::{lock, AppState};
use prisme_engine::Command;
use tauri::State;

/// The inputs `source` comes in on, kept for the next start: in use on the running device if
/// the turntables leave them free.
#[tauri::command]
pub fn set_live_input(
    state: State<'_, AppState>,
    source: LiveSourceDto,
    input: Option<InputChannelsDto>,
) -> AppResult<()> {
    state.update_preferences(|p| match source {
        LiveSourceDto::Mic => p.mixer.mic.input = input,
        LiveSourceDto::Aux => p.mixer.aux.input = input,
    });
    let info = lock(&state.session).as_ref().map(|s| s.info.clone());
    let channels = state.live_input(source, info.as_ref());
    state.send_if_running(Command::SetLiveInput { source: source.into(), channels: channels.map(Into::into) })
}

/// How far the music ducks under the microphone (dB); kept for the next start.
#[tauri::command]
pub fn set_talkover_depth(state: State<'_, AppState>, db: f32) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.talkover_depth_db = db.min(0.0));
    mixer::send_talkover(&state)
}
