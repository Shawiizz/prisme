//! A screen right under the one Prisme is on, as on a laptop with two screens: Prisme takes
//! both, the library above, the decks under the fingers on the touch screen below.

use serde::Serialize;
use tauri::{Monitor, PhysicalPosition, PhysicalSize, State, WebviewWindow};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// How far apart two screens may be laid out and still be one under the other (physical px).
const GAP_PX: i32 = 8;

/// Pretends there is a screen below, its top this far down the window (CSS px), and,
/// optionally, its scale against the main screen's (`560` or `560@0.8`): to try the layout
/// on a computer with one screen.
const FAKE_SPLIT: &str = "PRISME_FAKE_SPLIT";

/// The window spread over both screens.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpanDto {
    /// Where the screen below starts in the window (CSS px).
    split: f64,
    /// The window draws at the main screen's scale: what the screen below shows is scaled by
    /// this to look as Windows scales that screen (200 % below 250 %: 0.8).
    zoom: f64,
}

fn fake_span() -> Option<SpanDto> {
    let fake = std::env::var(FAKE_SPLIT).ok()?;
    let (split, zoom) = fake.split_once('@').unwrap_or((fake.as_str(), "1"));
    Some(SpanDto { split: split.parse().ok()?, zoom: zoom.parse().ok()? })
}

/// The screen the window is on, and the one under it: its top on the other's bottom (give or
/// take a few pixels), the two side by side for some width.
fn screens(window: &WebviewWindow) -> Option<(Monitor, Monitor)> {
    let main = window.current_monitor().ok()??;
    let (top, size) = (main.position(), main.size());
    let bottom = top.y + size.height as i32;
    let below = window.available_monitors().ok()?.into_iter().find(|m| {
        let (p, s) = (m.position(), m.size());
        (p.y - bottom).abs() <= GAP_PX && p.x < top.x + size.width as i32 && p.x + s.width as i32 > top.x
    })?;
    Some((main, below))
}

/// Whether there is a screen under the one Prisme is on.
#[tauri::command]
pub fn screen_below(window: WebviewWindow) -> bool {
    fake_span().is_some() || screens(&window).is_some()
}

/// Spreads the window over its screen and the one under it, or brings it back to its screen.
#[tauri::command]
pub fn span_screens(
    window: WebviewWindow,
    state: State<'_, AppState>,
    span: bool,
) -> AppResult<Option<SpanDto>> {
    state.update_settings(|p| p.ui.screen_below = span);
    let failed = |e: tauri::Error| AppError::Other(e.to_string());
    if let Some(fake) = fake_span() {
        return Ok(span.then_some(fake));
    }
    let Some((main, below)) = screens(&window) else {
        return Ok(None);
    };
    // a phone has one screen: none is ever found under it
    #[cfg(desktop)]
    if !span {
        window.maximize().map_err(failed)?;
        return Ok(None);
    }
    #[cfg(desktop)]
    window.unmaximize().map_err(failed)?;
    let (top, size) = (main.position(), main.size());
    window.set_position(PhysicalPosition::new(top.x, top.y)).map_err(failed)?;
    window.set_size(PhysicalSize::new(size.width, size.height + below.size().height)).map_err(failed)?;
    let scale = window.scale_factor().map_err(failed)?;
    Ok(Some(SpanDto { split: f64::from(size.height) / scale, zoom: below.scale_factor() / scale }))
}
