//! The decks on a screen of their own, as on a laptop with two screens or a touch screen beside
//! it: a window of their own, without borders, full screen on the screen the DJ chose; the main
//! window, apart from it, goes where it likes. Their screen unplugged, the decks go back into the
//! main window, and out to it again once it is plugged back.

use crate::error::AppResult;
use crate::state::AppState;
use serde::Serialize;
use tauri::{AppHandle, State};

/// The window of the decks.
pub const DECKS: &str = "decks";
/// Tells the main window whether the decks are in a window of their own.
pub const APART_EVENT: &str = "decks-apart";

/// A screen plugged in.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "Screen"))]
pub struct ScreenDto {
    /// The system's name of it, by which it is chosen.
    pub name: String,
    /// Its place and size on the desktop (physical px).
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// The main window is on it.
    pub main: bool,
}

/// The screens plugged in.
#[tauri::command]
pub fn list_screens(app: AppHandle) -> Vec<ScreenDto> {
    #[cfg(desktop)]
    return desktop::screens(&app);
    #[cfg(mobile)]
    {
        let _ = app;
        Vec::new()
    }
}

/// Whether the decks are in a window of their own.
#[tauri::command]
pub fn decks_apart(app: AppHandle) -> bool {
    use tauri::Manager;
    app.get_webview_window(DECKS).is_some()
}

/// The decks on the screen `screen` (its name), or back in the main window (`None`). Not on the
/// main thread: a window opened there would wait on it.
#[tauri::command]
pub async fn set_decks_screen(
    app: AppHandle,
    state: State<'_, AppState>,
    screen: Option<String>,
) -> AppResult<()> {
    state.update_settings(|p| p.ui.decks_screen = screen);
    #[cfg(desktop)]
    desktop::place(&app)?;
    #[cfg(mobile)]
    let _ = app;
    Ok(())
}

#[cfg(desktop)]
pub use desktop::{closed_by_the_dj, start};

#[cfg(desktop)]
mod desktop {
    use super::{ScreenDto, APART_EVENT, DECKS};
    use crate::error::{AppError, AppResult};
    use crate::state::AppState;
    use std::time::Duration;
    use tauri::{AppHandle, Emitter, Manager, Monitor, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
    use tracing::warn;

    /// How often the screens plugged in are looked at.
    const WATCH: Duration = Duration::from_secs(2);

    pub fn screens(app: &AppHandle) -> Vec<ScreenDto> {
        let main = app.get_webview_window("main").and_then(|w| w.current_monitor().ok().flatten());
        let main = main.and_then(|m| m.name().cloned());
        monitors(app)
            .into_iter()
            .filter_map(|m| {
                let name = m.name()?.clone();
                let (position, size) = (m.position(), m.size());
                Some(ScreenDto {
                    main: main.as_ref() == Some(&name),
                    name,
                    x: position.x,
                    y: position.y,
                    width: size.width,
                    height: size.height,
                })
            })
            .collect()
    }

    fn monitors(app: &AppHandle) -> Vec<Monitor> {
        app.available_monitors().inspect_err(|e| warn!("the screens not listed: {e}")).unwrap_or_default()
    }

    /// The main window as the DJ wants it at the start, full screen or filling its screen, and
    /// the decks on their screen, kept there as screens come and go.
    pub fn start(app: &AppHandle) {
        let full = app.state::<AppState>().preferences.read(|p| p.ui.start_fullscreen);
        if let Some(main) = app.get_webview_window("main").filter(|_| full) {
            // let go of filling the screen first, or Windows keeps that size full screen
            if let Err(e) = main.unmaximize().and_then(|()| main.set_fullscreen(true)) {
                warn!("not started full screen: {e}");
            }
        }
        let app = app.clone();
        std::thread::Builder::new()
            .name("prisme-screens".into())
            .spawn(move || loop {
                if let Err(e) = place(&app) {
                    warn!("the decks not put on their screen: {e}");
                }
                std::thread::sleep(WATCH);
            })
            .expect("cannot start the screens thread");
    }

    /// The decks where the DJ chose: in a window of their own on their screen if it is plugged
    /// in, else in the main window.
    pub fn place(app: &AppHandle) -> AppResult<()> {
        let chosen = app.state::<AppState>().preferences.read(|p| p.ui.decks_screen.clone());
        let screen = chosen.and_then(|name| monitors(app).into_iter().find(|m| m.name() == Some(&name)));
        let failed = |e: tauri::Error| AppError::Other(e.to_string());
        match (screen, app.get_webview_window(DECKS)) {
            (Some(screen), Some(window)) => {
                let on = window.current_monitor().map_err(failed)?.and_then(|m| m.name().cloned());
                if on.as_ref() != screen.name() {
                    fill(&window, &screen).map_err(failed)?;
                }
            }
            (Some(screen), None) => {
                let window =
                    WebviewWindowBuilder::new(app, DECKS, WebviewUrl::App("index.html?view=decks".into()))
                        .title("Prisme")
                        .decorations(false)
                        .visible(false)
                        .skip_taskbar(true)
                        .disable_drag_drop_handler()
                        .build()
                        .map_err(failed)?;
                fill(&window, &screen).map_err(failed)?;
                window.show().map_err(failed)?;
                tell(app, true);
            }
            (None, Some(window)) => {
                window.destroy().map_err(failed)?;
                tell(app, false);
            }
            (None, None) => {}
        }
        Ok(())
    }

    /// The window full screen on `screen`.
    fn fill(window: &WebviewWindow, screen: &Monitor) -> tauri::Result<()> {
        window.set_fullscreen(false)?;
        window.set_position(*screen.position())?;
        window.set_size(*screen.size())?;
        window.set_fullscreen(true)
    }

    fn tell(app: &AppHandle, apart: bool) {
        if let Err(e) = app.emit_to("main", APART_EVENT, apart) {
            warn!("the main window not told where the decks are: {e}");
        }
    }

    /// The window of the decks closed by the DJ (its keys, the system's): the decks back in the
    /// main window, until another screen is chosen.
    pub fn closed_by_the_dj(app: &AppHandle) {
        app.state::<AppState>().update_settings(|p| p.ui.decks_screen = None);
        tell(app, false);
    }
}
