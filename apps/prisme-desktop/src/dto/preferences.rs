//! The DJ's preferences as the settings show them, and what they offer.

use super::TargetDto;
use crate::preferences::{
    AutoDjPreferences, DeckCount, DjPreferences, LibraryPreferences, MixerPreferences, UiPreferences,
};
use crate::shortcuts::{self, Shortcut};
use crate::state::AppState;
use prisme_actions::scales::TEMPO_RANGES;
use prisme_control::Target;
use serde::Serialize;

/// What the settings show of the preferences, and what they offer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "Preferences"))]
pub struct PreferencesDto {
    pub dj: DjPreferences,
    pub library: LibraryPreferences,
    pub ui: UiPreferences,
    /// The keyboard's shortcuts in use: the keys the DJ gave, the defaults of the other actions.
    pub shortcuts: Vec<ShortcutDto>,
    /// The actions whose keys the DJ changed.
    pub changed: Vec<String>,
    pub choices: SettingChoicesDto,
}

/// A key of the keyboard bound to an action.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "Shortcut"))]
pub struct ShortcutDto {
    #[serde(flatten)]
    pub shortcut: Shortcut,
    /// It acts while typing and over the settings and the palette.
    pub everywhere: bool,
    /// Its action is of a deck in use, or of none.
    pub in_use: bool,
}

/// The values the settings offer where there are a few.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "SettingChoices"))]
pub struct SettingChoicesDto {
    pub tempo_ranges: [f64; 3],
    pub deck_counts: [usize; 2],
    pub talkover_depths: [f32; 4],
    pub languages: [&'static str; 2],
    /// The lengths the Auto DJ's mixes are offered at: in beats, and in seconds.
    pub fade_beats: [u32; 4],
    pub fade_seconds: [f64; 4],
}

impl PreferencesDto {
    pub fn of(state: &AppState) -> Self {
        let (dj, library, ui, keys) =
            state.preferences.read(|p| (p.dj, p.library.clone(), p.ui.clone(), p.keys.clone()));
        let in_use = |control: &str| control.parse::<Target>().is_ok_and(|t| t.in_use(dj.decks()));
        let shortcuts = shortcuts::effective(&keys)
            .into_iter()
            .map(|shortcut| ShortcutDto {
                everywhere: shortcuts::everywhere(&shortcut),
                in_use: in_use(&shortcut.control),
                shortcut,
            })
            .collect();
        let choices = SettingChoicesDto {
            tempo_ranges: TEMPO_RANGES,
            deck_counts: [DeckCount::Two.into(), DeckCount::Four.into()],
            talkover_depths: MixerPreferences::TALKOVER_DEPTHS,
            languages: UiPreferences::LANGUAGES,
            fade_beats: AutoDjPreferences::FADE_BEATS,
            fade_seconds: AutoDjPreferences::FADE_SECONDS,
        };
        Self { dj, library, ui, shortcuts, changed: keys.into_keys().collect(), choices }
    }
}

/// A key another action has, which a change of keys would take from it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "TakenKey"))]
pub struct TakenKeyDto {
    #[serde(flatten)]
    pub shortcut: Shortcut,
    /// The action that has it, as the interface names it.
    pub action: Option<TargetDto>,
}
