//! The actions the interface asks for, as a controller's are: by their target, in the order
//! the interface sent them.

use crate::actions::{Actions, Order};
use crate::commands::blocking;
use crate::error::AppResult;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// Performs the action `target` driven to `value` as the interface asks (see
/// `Actions::perform_asked`).
#[tauri::command]
pub async fn perform_action(
    app: AppHandle,
    target: String,
    value: Option<f32>,
    confirmed: bool,
    track: Option<PathBuf>,
    order: Order,
) -> AppResult<()> {
    blocking(move || app.state::<Actions>().perform_asked(&app, &target, value, confirmed, track, order))
        .await
}

/// The interface's press of the key `target` lost: let go, without what its release does.
#[tauri::command]
pub async fn let_go_key(app: AppHandle, target: String, order: Order) -> AppResult<()> {
    blocking(move || app.state::<Actions>().lose_key(&target, order)).await
}
