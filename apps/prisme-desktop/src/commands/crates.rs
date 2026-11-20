//! The crates: named sets of tracks, or rules for the smart ones. The side panel and the list
//! shown hear of a change with the status stream.

use crate::actions::browse;
use crate::actions::Order;
use crate::commands::library::in_turn;
use crate::dto::SmartRulesDto;
use crate::error::AppResult;
use prisme_library::CrateId;
use tauri::AppHandle;

/// A crate named `name`, shown: its id.
#[tauri::command]
pub async fn create_crate(app: AppHandle, name: String, order: Order) -> AppResult<i64> {
    in_turn(app, order, move |state, db| Ok(browse::create_crate(state, db, &name, None)?.0)).await
}

/// A smart crate named `name`, picking its tracks by `rules`, shown: its id.
#[tauri::command]
pub async fn create_smart_crate(
    app: AppHandle,
    name: String,
    rules: SmartRulesDto,
    order: Order,
) -> AppResult<i64> {
    in_turn(app, order, move |state, db| {
        let rules = rules.checked()?;
        Ok(browse::create_crate(state, db, &name, Some(&rules))?.0)
    })
    .await
}

/// The smart crate `id` named `name` and picking its tracks by `rules`: both at once, or neither.
#[tauri::command]
pub async fn update_smart_crate(
    app: AppHandle,
    id: i64,
    name: String,
    rules: SmartRulesDto,
    order: Order,
) -> AppResult<()> {
    in_turn(app, order, move |state, db| {
        let rules = rules.checked()?;
        browse::change_crate(state, db, CrateId(id), &name, Some(&rules))
    })
    .await
}

#[tauri::command]
pub async fn rename_crate(app: AppHandle, id: i64, name: String, order: Order) -> AppResult<()> {
    in_turn(app, order, move |state, db| browse::change_crate(state, db, CrateId(id), &name, None)).await
}

#[tauri::command]
pub async fn delete_crate(app: AppHandle, id: i64, order: Order) -> AppResult<()> {
    in_turn(app, order, move |state, db| browse::delete_crate(state, db, CrateId(id))).await
}

/// Puts the tracks at `paths` (else those selected) in the crate `id`; how many were not in it.
#[tauri::command]
pub async fn add_to_crate(
    app: AppHandle,
    id: i64,
    paths: Option<Vec<String>>,
    order: Order,
) -> AppResult<usize> {
    in_turn(app, order, move |state, db| {
        let paths = browse::tracks(state, paths);
        browse::add_to_crate(state, db, CrateId(id), &paths)
    })
    .await
}
