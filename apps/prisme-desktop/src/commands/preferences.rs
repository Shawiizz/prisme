use crate::commands::audio::start_audio;
use crate::commands::blocking;
use crate::commands::live::TALKOVER_DEPTHS;
use crate::error::{AppError, AppResult};
use crate::library_service::LibraryService;
use crate::preferences::{
    DeckCount, DjPreferences, KeyPreference, LibraryPreferences, Preferences, UiPreferences, VinylSync,
};
use crate::state::{lock, AppState};
use crate::stems_service::StemsService;
use crate::tag_writer::TagWriter;
use prisme_actions::scales::TEMPO_RANGES;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

/// What the settings show of the preferences, and what they offer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferencesDto {
    pub dj: DjPreferences,
    pub library: LibraryPreferences,
    pub ui: UiPreferences,
    pub keys: BTreeMap<String, Vec<KeyPreference>>,
    pub choices: SettingChoicesDto,
}

/// The values the settings offer where there are a few.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingChoicesDto {
    pub tempo_ranges: [f64; 3],
    pub deck_counts: [usize; 2],
    pub talkover_depths: [f32; 4],
    pub languages: [&'static str; 2],
}

impl PreferencesDto {
    pub fn of(state: &AppState) -> Self {
        let (dj, library, ui, keys) =
            state.preferences.read(|p| (p.dj, p.library.clone(), p.ui.clone(), p.keys.clone()));
        let choices = SettingChoicesDto {
            tempo_ranges: TEMPO_RANGES,
            deck_counts: [DeckCount::Two.into(), DeckCount::Four.into()],
            talkover_depths: TALKOVER_DEPTHS,
            languages: UiPreferences::LANGUAGES,
        };
        Self { dj, library, ui, keys, choices }
    }
}

#[tauri::command]
pub fn get_preferences(state: State<'_, AppState>) -> PreferencesDto {
    PreferencesDto::of(&state)
}

/// One setting changed in the settings (the quantize and the tempo range are the actions';
/// the music folders have their own commands).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "setting", content = "value", rename_all = "camelCase")]
pub enum Setting {
    VinylSync(VinylSync),
    AutoGain(bool),
    DeckCount(usize),
    KnobMagnet(bool),
    LockControls(bool),
    CueOnLoad(bool),
    ProtectPlaying(bool),
    HiddenColumns(Vec<String>),
    WriteTags(bool),
    ReplaceTags(bool),
    PauseWhilePlaying(bool),
    StemsOnLoad(bool),
    StemsGpu(bool),
    StemsFast(bool),
    Language(Option<String>),
    PhoneTouch(Option<bool>),
}

/// What a setting changed does beyond the preferences.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Then {
    Nothing,
    /// The tracks' levels follow.
    Levels,
    /// Another number of decks: another engine.
    Decks,
    /// The tempos and keys of other programs are to be written over.
    Retag,
    WriteTags,
    /// Other separation models take over.
    Rechoose,
}

impl Setting {
    /// Puts the setting in `p`, unless it is not one the settings offer.
    fn put(self, p: &mut Preferences) -> AppResult<Then> {
        match self {
            Setting::VinylSync(sync) => p.dj.vinyl_sync = sync,
            Setting::AutoGain(on) => {
                p.dj.auto_gain = on;
                return Ok(Then::Levels);
            }
            Setting::DeckCount(count) => {
                let count = [DeckCount::Two, DeckCount::Four]
                    .into_iter()
                    .find(|c| usize::from(*c) == count)
                    .ok_or_else(|| AppError::Other(format!("no {count} decks")))?;
                let then = if count == p.dj.deck_count { Then::Nothing } else { Then::Decks };
                p.dj.deck_count = count;
                return Ok(then);
            }
            Setting::KnobMagnet(on) => p.dj.knob_magnet = on,
            Setting::LockControls(on) => p.dj.lock_controls = on,
            Setting::CueOnLoad(on) => p.dj.cue_on_load = on,
            Setting::ProtectPlaying(on) => p.dj.protect_playing = on,
            Setting::HiddenColumns(columns) => p.library.hidden_columns = columns,
            Setting::WriteTags(on) => {
                p.library.write_tags = on;
                return Ok(if on { Then::WriteTags } else { Then::Nothing });
            }
            Setting::ReplaceTags(on) => {
                let then = if on && !p.library.replace_tags { Then::Retag } else { Then::Nothing };
                p.library.replace_tags = on;
                return Ok(then);
            }
            Setting::PauseWhilePlaying(on) => p.library.pause_while_playing = on,
            Setting::StemsOnLoad(on) => p.library.separate_on_load = Some(on),
            Setting::StemsGpu(on) => p.library.stems_gpu = on,
            Setting::StemsFast(on) => {
                let then = if on == p.library.stems_fast { Then::Nothing } else { Then::Rechoose };
                p.library.stems_fast = on;
                return Ok(then);
            }
            Setting::Language(language) => {
                if language.as_deref().is_some_and(|l| !UiPreferences::LANGUAGES.contains(&l)) {
                    return Err(AppError::Other(format!("no language {language:?}")));
                }
                p.ui.language = language;
            }
            Setting::PhoneTouch(touch) => p.ui.phone_touch = touch,
        }
        Ok(Then::Nothing)
    }
}

/// Off the main thread: another number of decks restarts the audio, other tags chosen may mark
/// every track of the library to be tagged again.
#[tauri::command]
pub async fn set_preference(app: AppHandle, setting: Setting) -> AppResult<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        let then = change(&state, setting)?;
        match then {
            Then::Nothing => {}
            Then::Levels => state.apply_levels()?,
            Then::Decks => {
                // the audio starts again with them, the tracks staying on the decks there still are
                let decks = state.preferences.read(|p| p.dj.decks());
                lock(&state.tracks)[decks..].fill_with(|| None);
                let running = lock(&state.session).as_ref().map(|s| s.settings.clone());
                if let Some(settings) = running {
                    start_audio(&state, &settings)?;
                }
                state.apply_levels()?;
            }
            Then::Retag => app.state::<Arc<LibraryService>>().db.retag_all()?,
            Then::WriteTags => app.state::<TagWriter>().wake(),
            Then::Rechoose => app.state::<StemsService>().rechosen(),
        }
        Ok(())
    })
    .await
}

/// `setting` put in the preferences of `state`, the interface hearing of it.
fn change(state: &AppState, setting: Setting) -> AppResult<Then> {
    let mut put = Ok(Then::Nothing);
    state.update_settings(|p| put = setting.put(p));
    put
}

/// The keyboard keys the DJ changed, by action.
#[tauri::command]
pub fn set_key_preferences(state: State<'_, AppState>, keys: BTreeMap<String, Vec<KeyPreference>>) {
    state.update_settings(|p| p.keys = keys);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> AppState {
        AppState::new(prisme_session::JsonFile::in_memory(Default::default()))
    }

    fn setting(json: serde_json::Value) -> Setting {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn a_setting_changes_itself_alone() {
        let state = state();
        let before = state.preferences.get();
        change(&state, setting(serde_json::json!({ "setting": "cueOnLoad", "value": true }))).unwrap();
        let after = state.preferences.get();
        assert!(after.dj.cue_on_load);
        assert_eq!(DjPreferences { cue_on_load: false, ..after.dj }, before.dj, "the rest as it was");
        assert_eq!(after.library, before.library);
    }

    #[test]
    fn a_setting_the_settings_do_not_offer_is_refused() {
        let state = state();
        assert!(change(&state, Setting::DeckCount(3)).is_err());
        assert!(change(&state, Setting::Language(Some("de".into()))).is_err());
        assert_eq!(state.preferences.get(), Preferences::default());
        assert_eq!(change(&state, Setting::DeckCount(4)).unwrap(), Then::Decks);
        assert_eq!(change(&state, Setting::DeckCount(4)).unwrap(), Then::Nothing);
        assert_eq!(change(&state, Setting::Language(None)).unwrap(), Then::Nothing);
    }

    #[test]
    fn what_a_setting_does_beyond_itself_happens_once_it_changes() {
        let state = state();
        let fast = state.preferences.read(|p| p.library.stems_fast);
        assert_eq!(change(&state, Setting::StemsFast(fast)).unwrap(), Then::Nothing);
        assert_eq!(change(&state, Setting::StemsFast(!fast)).unwrap(), Then::Rechoose);
        assert_eq!(change(&state, Setting::ReplaceTags(true)).unwrap(), Then::Retag);
        assert_eq!(change(&state, Setting::ReplaceTags(true)).unwrap(), Then::Nothing);
    }

    #[test]
    fn the_interface_hears_of_a_setting_changed_with_the_rest_of_the_settings() {
        let state = state();
        let before = state.revision();
        change(&state, Setting::PhoneTouch(Some(false))).unwrap();
        let moved = state.moved_since(before);
        assert!(moved.contains(&crate::state::Section::Settings), "{moved:?}");
        let shown = PreferencesDto::of(&state);
        assert_eq!(shown.ui.phone_touch, Some(false));
        assert_eq!(shown.choices.deck_counts, [2, 4]);
    }
}
