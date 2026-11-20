use crate::auto_dj::AutoDj;
use crate::dto::AutoDjStatusDto;
use crate::preferences::AutoDjPreferences;
use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub fn auto_dj_status(auto_dj: State<'_, AutoDj>) -> AutoDjStatusDto {
    auto_dj.status()
}

/// How long a mix lasts, kept for the next start.
#[tauri::command]
pub fn set_auto_dj_fade(
    state: State<'_, AppState>,
    auto_dj: State<'_, AutoDj>,
    beats: u32,
    seconds: f64,
) -> AutoDjStatusDto {
    let fade = AutoDjPreferences { fade_beats: beats, fade_s: seconds }.bounded();
    state.update_preferences(|p| p.auto_dj = fade);
    auto_dj.set_fade(fade);
    auto_dj.status()
}
