use crate::commands::audio::start_audio;
use crate::commands::blocking;
use crate::dto::{PreferencesDto, TakenKeyDto, TargetDto};
use crate::error::{AppError, AppResult};
use crate::library_service::LibraryService;
use crate::preferences::{DeckCount, DecksLayout, KeyPreference, Preferences, UiPreferences, VinylSync};
use crate::shortcuts;
use crate::state::{lock, AppState};
use crate::stems_service::{self, StemsService};
use crate::tag_writer::TagWriter;
use prisme_control::Target;
use serde::Deserialize;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

#[tauri::command]
pub fn get_preferences(state: State<'_, AppState>) -> PreferencesDto {
    PreferencesDto::of(&state)
}

/// One setting changed in the settings (the quantize and the tempo range are the actions';
/// the music folders have their own commands).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "setting", content = "value", rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Setting {
    VinylSync(VinylSync),
    AutoGain(bool),
    DeckCount(usize),
    KnobMagnet(bool),
    LockControls(bool),
    CueOnLoad(bool),
    ProtectPlaying(bool),
    /// A column of the track table shown, or hidden.
    Column {
        column: String,
        shown: bool,
    },
    WriteTags(bool),
    ReplaceTags(bool),
    PauseWhilePlaying(bool),
    StemsOnLoad(bool),
    StemsGpu(bool),
    StemsFast(bool),
    Language(Option<String>),
    PhoneTouch(Option<bool>),
    /// The main window full screen at the next start.
    StartFullscreen(bool),
    DecksLayout(DecksLayout),
    /// Prisme mixes on the sound card of a controller with no mixer of its own.
    AllInOneMixes(bool),
    /// New versions downloaded and put in place by themselves (a computer's).
    AutomaticUpdates(bool),
}

/// What a setting changed does beyond the preferences.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Then {
    Nothing,
    /// The tracks' levels follow.
    Levels,
    /// Another number of decks: another engine.
    Decks,
    /// The decks heard elsewhere: the routing follows.
    Route,
    /// The tempos and keys of other programs are to be written over.
    Retag,
    WriteTags,
    /// Other separation models take over.
    Rechoose,
    /// A new version found is downloaded now.
    LookForUpdate,
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
            Setting::Column { column, shown } => {
                let hidden = &mut p.library.hidden_columns;
                if shown {
                    hidden.retain(|hidden| *hidden != column);
                } else if !hidden.contains(&column) {
                    hidden.push(column);
                }
            }
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
                return Ok(if stems_service::choose_fast(p, on) { Then::Rechoose } else { Then::Nothing });
            }
            Setting::Language(language) => {
                if language.as_deref().is_some_and(|l| !UiPreferences::LANGUAGES.contains(&l)) {
                    return Err(AppError::Other(format!("no language {language:?}")));
                }
                p.ui.language = language;
            }
            Setting::PhoneTouch(touch) => p.ui.phone_touch = touch,
            Setting::StartFullscreen(on) => p.ui.start_fullscreen = on,
            Setting::DecksLayout(layout) => p.ui.decks_layout = layout,
            Setting::AllInOneMixes(on) => {
                p.mixer.all_in_one_mixes = on;
                return Ok(Then::Route);
            }
            Setting::AutomaticUpdates(on) => {
                p.updates.automatic = on;
                return Ok(if on { Then::LookForUpdate } else { Then::Nothing });
            }
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
                let decks = state.decks_in_use();
                lock(&state.tracks)[decks..].fill_with(|| None);
                let running = lock(&state.session).as_ref().map(|s| s.settings.clone());
                if let Some(settings) = running {
                    start_audio(&state, &settings)?;
                }
                state.apply_levels()?;
            }
            Then::Retag => {
                app.state::<Arc<LibraryService>>().db.retag_all()?;
                app.state::<TagWriter>().wake();
            }
            Then::WriteTags => app.state::<TagWriter>().wake(),
            Then::Route => crate::actions::mixer::route(&state)?,
            Then::Rechoose => app.state::<StemsService>().rechosen(),
            Then::LookForUpdate => {
                #[cfg(desktop)]
                let _ = app.state::<crate::updates::Updates>().look();
            }
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

/// `keys` given to `control` (none: those Prisme gives it), taken from the actions that have
/// them once the DJ says so (`take`): until then they are told back, nothing changed.
#[tauri::command]
pub fn set_keys(
    state: State<'_, AppState>,
    control: String,
    keys: Option<Vec<KeyPreference>>,
    take: bool,
) -> AppResult<Vec<TakenKeyDto>> {
    give_keys(&state, &control, keys, take)
}

fn give_keys(
    state: &AppState,
    control: &str,
    keys: Option<Vec<KeyPreference>>,
    take: bool,
) -> AppResult<Vec<TakenKeyDto>> {
    if !shortcuts::keyed(control) {
        return Err(AppError::Other(format!("no key presses {control}")));
    }
    let keys = keys.unwrap_or_else(|| shortcuts::default_keys(control));
    let mut taken = Vec::new();
    state.update_settings(|p| {
        taken = shortcuts::conflicts(&shortcuts::effective(&p.keys), control, &keys);
        if take || taken.is_empty() {
            p.keys = shortcuts::assign(&p.keys, control, &keys);
        }
    });
    if take {
        return Ok(Vec::new());
    }
    let decks = state.decks_in_use();
    let action = |control: &str| control.parse::<Target>().ok().map(|t| TargetDto::of(&t, decks));
    Ok(taken
        .into_iter()
        .map(|shortcut| TakenKeyDto { action: action(&shortcut.control), shortcut })
        .collect())
}

/// Every action back to the keys Prisme gives it.
#[tauri::command]
pub fn reset_keys(state: State<'_, AppState>) {
    state.update_settings(|p| p.keys.clear());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preferences::DjPreferences;

    fn state() -> AppState {
        AppState::new(prisme_session::JsonFile::in_memory(Default::default()))
    }

    fn setting(json: serde_json::Value) -> Setting {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn keys_another_action_has_are_told_until_the_dj_says_to_take_them() {
        let state = state();
        let a = KeyPreference { code: "KeyA".into(), ctrl: false, shift: false };
        let taken = give_keys(&state, "deck1.play", Some(vec![a.clone()]), false).unwrap();
        assert_eq!(taken.iter().map(|s| s.shortcut.control.as_str()).collect::<Vec<_>>(), ["deck1.cue"]);
        assert_eq!(
            taken[0].action.as_ref().map(|a| a.label.clone()),
            Some("deck1.cue".parse::<Target>().unwrap().label())
        );
        assert!(state.preferences.read(|p| p.keys.is_empty()), "nothing changed yet");
        assert_eq!(give_keys(&state, "deck1.play", Some(vec![a.clone()]), true).unwrap(), []);
        let shown = PreferencesDto::of(&state);
        assert_eq!(shown.changed, ["deck1.cue", "deck1.play"]);
        let play: Vec<_> = shown.shortcuts.iter().filter(|s| s.shortcut.control == "deck1.play").collect();
        assert_eq!(play.len(), 1);
        assert_eq!(play[0].shortcut.key, a);
        give_keys(&state, "deck1.cue", None, true).unwrap();
        give_keys(&state, "deck1.play", None, false).unwrap();
        assert!(state.preferences.read(|p| p.keys.is_empty()), "both back to their defaults");
        assert!(give_keys(&state, "mixer.crossfader", None, false).is_err(), "no key presses a fader");
    }

    #[test]
    fn a_key_taken_from_an_action_left_out_of_the_list_says_what_it_does() {
        let state = state();
        let r = KeyPreference { code: "KeyR".into(), ctrl: true, shift: true };
        give_keys(&state, "deck1.loop.2", Some(vec![r.clone()]), true).unwrap();
        let taken = give_keys(&state, "deck1.play", Some(vec![r]), false).unwrap();
        let loop_2: Target = "deck1.loop.2".parse().unwrap();
        assert_eq!(taken[0].action.as_ref().map(|a| a.label.clone()), Some(loop_2.label()));
    }

    #[test]
    fn the_keys_of_decks_not_in_use_do_nothing() {
        let state = state();
        let in_use = |control: &str| {
            PreferencesDto::of(&state)
                .shortcuts
                .iter()
                .find(|s| s.shortcut.control == control)
                .unwrap()
                .in_use
        };
        assert!(in_use("deck2.play") && !in_use("deck3.play") && in_use("library.load"));
        change(&state, Setting::DeckCount(4)).unwrap();
        assert!(in_use("deck3.play"));
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
    fn a_column_is_shown_or_hidden_alone() {
        let state = state();
        let column = |column: &str, shown| Setting::Column { column: column.into(), shown };
        let hidden = || state.preferences.read(|p| p.library.hidden_columns.clone());
        change(&state, column("album", false)).unwrap();
        change(&state, column("year", false)).unwrap();
        change(&state, column("album", false)).unwrap();
        assert_eq!(hidden(), ["album", "year"]);
        change(&state, column("album", true)).unwrap();
        assert_eq!(hidden(), ["year"]);
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
        change(&state, Setting::StartFullscreen(true)).unwrap();
        change(&state, Setting::DecksLayout(DecksLayout::Lanes)).unwrap();
        let moved = state.moved_since(before);
        assert!(moved.contains(&crate::state::Section::Settings), "{moved:?}");
        let shown = PreferencesDto::of(&state);
        assert_eq!((shown.ui.phone_touch, shown.ui.start_fullscreen), (Some(false), true));
        assert_eq!(shown.ui.decks_layout, DecksLayout::Lanes);
        assert_eq!(shown.choices.deck_counts, [2, 4]);
    }
}
