use crate::auto_dj::AutoDj;
use crate::dto::AutoDjStatusDto;
use crate::preferences::AutoDjPreferences;
use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub fn auto_dj_status(auto_dj: State<'_, AutoDj>) -> AutoDjStatusDto {
    auto_dj.status()
}

/// On, off, or on again after the DJ took over.
#[tauri::command]
pub fn toggle_auto_dj(auto_dj: State<'_, AutoDj>) -> AutoDjStatusDto {
    auto_dj.toggle();
    auto_dj.status()
}

/// Mixes into the next track at once.
#[tauri::command]
pub fn auto_dj_fade_now(auto_dj: State<'_, AutoDj>) -> AutoDjStatusDto {
    auto_dj.fade_now();
    auto_dj.status()
}

/// Leaves the next track out of the prepare list: the one after comes next.
#[tauri::command]
pub fn auto_dj_skip(auto_dj: State<'_, AutoDj>) -> AutoDjStatusDto {
    auto_dj.skip();
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
