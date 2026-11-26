//! The prepare list: the tracks lined up for the next minutes of the set.

use crate::actions::browse;
use crate::actions::Order;
use crate::commands::library::in_turn;
use crate::error::AppResult;
use crate::stems_service::StemsService;
use tauri::{AppHandle, Manager};

/// Puts the tracks at `paths` (else those selected) at the end of the list; how many were not in
/// it. They are separated in the background if the DJ chose so: ready before the set.
#[tauri::command]
pub async fn add_to_prepare(app: AppHandle, paths: Option<Vec<String>>, order: Order) -> AppResult<usize> {
    let stems = app.clone();
    in_turn(app, order, move |state, db| {
        let paths = browse::tracks(state, paths);
        browse::add_to_prepare(state, db, Some(&stems.state::<StemsService>()), &paths)
    })
    .await
}

/// Puts the tracks at `paths` (else those selected) before the track at `before` (`None`: at the
/// end) of the list shown, the prepare list or a crate filled by hand, in their order, adding
/// those not in it.
#[tauri::command]
pub async fn place_in_list(
    app: AppHandle,
    paths: Option<Vec<String>>,
    before: Option<String>,
    order: Order,
) -> AppResult<()> {
    in_turn(app, order, move |state, db| {
        let paths = browse::tracks(state, paths);
        browse::place_in_list(state, db, &paths, before.as_deref())
    })
    .await
}
