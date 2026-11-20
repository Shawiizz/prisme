//! A deck's sampler (see `sampler`).

use super::blocking;
use crate::actions::sampler;
use crate::error::AppResult;
use crate::state::AppState;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// Puts the sound of a file the DJ chose in a slot; kept for the next start.
#[tauri::command]
pub async fn load_sample(app: AppHandle, deck: usize, slot: usize, path: String) -> AppResult<()> {
    blocking(move || sampler::load_sample(&app.state::<AppState>(), deck, slot, PathBuf::from(path))).await
}
