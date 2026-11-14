//! A screen right under the one Prisme is on, as on a laptop with two screens: Prisme takes
//! both, the library above, the decks under the fingers on the touch screen below.

use tauri::{Monitor, PhysicalPosition, PhysicalSize, WebviewWindow};

use crate::error::{AppError, AppResult};

/// Pretends there is a screen below, its top this far down the window (CSS px): to try the
/// layout on a computer with one screen.
const FAKE_SPLIT: &str = "PRISME_FAKE_SPLIT";

fn fake_split() -> Option<f64> {
    std::env::var(FAKE_SPLIT).ok()?.parse().ok()
}

/// The screen the window is on, and the one under it: its top on the other's bottom, the two
/// side by side for some width.
fn screens(window: &WebviewWindow) -> Option<(Monitor, Monitor)> {
    let main = window.current_monitor().ok()??;
    let (top, size) = (main.position(), main.size());
    let bottom = top.y + size.height as i32;
    let below = window.available_monitors().ok()?.into_iter().find(|m| {
        let (p, s) = (m.position(), m.size());
        p.y == bottom && p.x < top.x + size.width as i32 && p.x + s.width as i32 > top.x
    })?;
    Some((main, below))
}

/// Whether there is a screen under the one Prisme is on.
#[tauri::command]
pub fn screen_below(window: WebviewWindow) -> bool {
    fake_split().is_some() || screens(&window).is_some()
}

/// Spreads the window over its screen and the one under it, or brings it back to its screen.
/// Spread, returns where the screen below starts in the window (CSS px).
#[tauri::command]
pub fn span_screens(window: WebviewWindow, span: bool) -> AppResult<Option<f64>> {
    let failed = |e: tauri::Error| AppError::Other(e.to_string());
    if let Some(split) = fake_split() {
        return Ok(span.then_some(split));
    }
    let Some((main, below)) = screens(&window) else {
        return Ok(None);
    };
    if !span {
        window.maximize().map_err(failed)?;
        return Ok(None);
    }
    window.unmaximize().map_err(failed)?;
    let (top, size) = (main.position(), main.size());
    window.set_position(PhysicalPosition::new(top.x, top.y)).map_err(failed)?;
    window.set_size(PhysicalSize::new(size.width, size.height + below.size().height)).map_err(failed)?;
    Ok(Some(f64::from(size.height) / window.scale_factor().map_err(failed)?))
}
