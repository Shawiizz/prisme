use crate::auto_dj::AutoDj;
use crate::dto::AutoDjStatusDto;
use crate::preferences::AutoDjPreferences;
use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub fn auto_dj_status(auto_dj: State<'_, AutoDj>) -> AutoDjStatusDto {
    auto_dj.status()
}

/// How long a mix lasts between tracks with a beat grid (`beats`), or when one has none
/// (`seconds`), the other as it is; kept for the next start.
#[tauri::command]
pub fn set_auto_dj_fade(
    state: State<'_, AppState>,
    auto_dj: State<'_, AutoDj>,
    beats: Option<u32>,
    seconds: Option<f64>,
) -> AutoDjStatusDto {
    let mut fade = AutoDjPreferences::default();
    state.update_preferences(|p| {
        p.auto_dj = AutoDjPreferences {
            fade_beats: beats.unwrap_or(p.auto_dj.fade_beats),
            fade_s: seconds.unwrap_or(p.auto_dj.fade_s),
        }
        .bounded();
        fade = p.auto_dj;
    });
    auto_dj.set_fade(fade);
    auto_dj.status()
}
