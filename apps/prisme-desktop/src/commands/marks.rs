//! What the DJ marks tracks with: a colour, a rating, a genre written into their files.

use crate::actions::browse;
use crate::actions::Order;
use crate::commands::library::in_turn;
use crate::error::{AppError, AppResult};
use crate::tag_writer::TagWriter;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// Colours the tracks at `paths` (else those selected); `None` takes the colour off.
#[tauri::command]
pub async fn set_track_color(
    app: AppHandle,
    paths: Option<Vec<String>>,
    color: Option<u32>,
    order: Order,
) -> AppResult<()> {
    in_turn(app, order, move |state, db| {
        let paths = browse::tracks(state, paths);
        browse::set_color(state, db, &paths, color)
    })
    .await
}

/// Rates the track at `path` `stars` (1 to 5): the stars it has already take its rating off.
#[tauri::command]
pub async fn rate_track(app: AppHandle, path: String, stars: u8, order: Order) -> AppResult<()> {
    in_turn(app, order, move |state, db| browse::rate(state, db, &path, stars)).await
}

/// Writes `genre` into the tags of the files at `paths` (else of those selected; empty or `None`:
/// takes it off); the rows follow as each file is written.
#[tauri::command]
pub async fn set_track_genre(
    app: AppHandle,
    paths: Option<Vec<String>>,
    genre: Option<String>,
    order: Order,
) -> AppResult<()> {
    let writer = app.clone();
    let paths = in_turn(app, order, move |state, _| Ok(browse::tracks(state, paths))).await?;
    let genre = genre.map(|g| g.trim().to_owned()).filter(|g| !g.is_empty());
    let answer = writer.state::<TagWriter>().set_genre(paths.into_iter().map(PathBuf::from).collect(), genre);
    let failed = tauri::async_runtime::spawn_blocking(move || answer.recv().unwrap_or_default())
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;
    if failed.is_empty() {
        Ok(())
    } else {
        Err(AppError::GenreNotWritten(failed))
    }
}
