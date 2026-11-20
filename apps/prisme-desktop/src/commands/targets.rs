//! The actions a key or a controller can drive, as the interface lists them: for the keys, the
//! controllers' mappings and the command palette.

use crate::state::AppState;
use prisme_control::{Kind, Label, Target};
use serde::Serialize;
use tauri::State;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetDto {
    pub id: String,
    pub kind: Kind,
    /// The deck it acts on: a deck's, or a load's.
    pub deck: Option<usize>,
    pub group: Label,
    pub label: Label,
    /// Of a deck in use, or of none.
    pub in_use: bool,
}

impl TargetDto {
    pub fn of(target: &Target, decks: usize) -> Self {
        Self {
            id: target.to_string(),
            kind: target.kind(),
            deck: target.deck(),
            group: target.group(),
            label: target.label(),
            in_use: target.in_use(decks),
        }
    }
}

/// The decks in use.
pub fn decks(state: &AppState) -> usize {
    state.preferences.read(|p| p.dj.decks())
}

/// Every action listed, in the order shown.
#[tauri::command]
pub fn list_targets(state: State<'_, AppState>) -> Vec<TargetDto> {
    let decks = decks(&state);
    Target::all().iter().map(|target| TargetDto::of(target, decks)).collect()
}
