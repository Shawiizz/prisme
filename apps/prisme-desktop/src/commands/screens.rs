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
/// What the decks' window is called, in the taskbar and among the windows.
const DECKS_TITLE: &str = "Prisme Decks";

/// A screen plugged in.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "Screen"))]
pub struct ScreenDto {
    /// What the system knows the screen itself by (see `screen_id`), by which it is chosen.
    pub id: String,
    /// The system's name of where it is plugged (`\\.\DISPLAY2`).
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

/// The decks on the screen `screen` (its id), or back in the main window (`None`). Not on the
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

/// The decks' window, if they have one, in front of the others: a program put over it, or the
/// window minimised.
#[tauri::command]
pub fn show_decks(app: AppHandle) -> AppResult<()> {
    #[cfg(desktop)]
    desktop::bring_to_front(&app)?;
    #[cfg(mobile)]
    let _ = app;
    Ok(())
}

#[cfg(desktop)]
pub use desktop::{closed_by_the_dj, start};

#[cfg(desktop)]
mod desktop {
    use super::{ScreenDto, APART_EVENT, DECKS, DECKS_TITLE};
    use crate::error::{AppError, AppResult};
    use crate::state::AppState;
    use std::time::Duration;
    use tauri::image::Image;
    use tauri::{AppHandle, Emitter, Manager, Monitor, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
    use tracing::warn;

    /// How often the screens plugged in are looked at.
    const WATCH: Duration = Duration::from_secs(2);

    pub fn screens(app: &AppHandle) -> Vec<ScreenDto> {
        let main = app.get_webview_window("main").and_then(|w| w.current_monitor().ok().flatten());
        let main = main.as_ref().and_then(screen_id);
        monitors(app)
            .into_iter()
            .filter_map(|m| {
                let id = screen_id(&m)?;
                let (position, size) = (m.position(), m.size());
                Some(ScreenDto {
                    main: main.as_ref() == Some(&id),
                    id,
                    name: m.name()?.clone(),
                    x: position.x,
                    y: position.y,
                    width: size.width,
                    height: size.height,
                })
            })
            .collect()
    }

    /// What the system knows `monitor` by: on Windows the monitor's device (its model and the
    /// port it is on, kept as the displays are numbered anew), elsewhere the name of its output
    /// (`HDMI-1`), as it is told apart there.
    fn screen_id(monitor: &Monitor) -> Option<String> {
        let name = monitor.name()?;
        #[cfg(windows)]
        return super::windows_screens::device_of(name);
        #[cfg(not(windows))]
        Some(name.clone())
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
        let screen =
            chosen.and_then(|id| monitors(app).into_iter().find(|m| screen_id(m) == Some(id.clone())));
        let failed = |e: tauri::Error| AppError::Other(e.to_string());
        match (screen, app.get_webview_window(DECKS)) {
            (Some(screen), Some(window)) => {
                let on = window.current_monitor().map_err(failed)?.as_ref().and_then(screen_id);
                if on != screen_id(&screen) {
                    fill(&window, &screen).map_err(failed)?;
                }
            }
            (Some(screen), None) => {
                let window =
                    WebviewWindowBuilder::new(app, DECKS, WebviewUrl::App("index.html?view=decks".into()))
                        .title(DECKS_TITLE)
                        .icon(decks_icon())
                        .map_err(failed)?
                        .decorations(false)
                        .visible(false)
                        .disable_drag_drop_handler()
                        .build()
                        .map_err(failed)?;
                fill(&window, &screen).map_err(failed)?;
                #[cfg(windows)]
                button_apart(&window);
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

    /// Prisme's icon with a record in its corner, to tell the decks' window from the main one.
    fn decks_icon() -> Image<'static> {
        Image::from_bytes(include_bytes!("../../icons/decks.png")).expect("the decks' icon is a PNG")
    }

    /// The decks' window, a button of its own in the taskbar, beside Prisme's: Windows groups
    /// an application's windows under one button otherwise, with Prisme's icon.
    #[cfg(windows)]
    fn button_apart(window: &WebviewWindow) {
        let apart = window.hwnd().map_err(|e| e.to_string()).and_then(|hwnd| {
            prisme_update::windows::taskbar_button_apart(
                hwnd.0 as isize,
                &prisme_update::Identity::current(),
                DECKS,
            )
            .map_err(|e| e.to_string())
        });
        if let Err(e) = apart {
            warn!("the decks' window grouped with the main one in the taskbar: {e}");
        }
    }

    /// The decks' window, if there is one, shown in front of the others.
    pub fn bring_to_front(app: &AppHandle) -> AppResult<()> {
        let Some(window) = app.get_webview_window(DECKS) else { return Ok(()) };
        let failed = |e: tauri::Error| AppError::Other(e.to_string());
        window.unminimize().map_err(failed)?;
        window.show().map_err(failed)?;
        window.set_focus().map_err(failed)
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

#[cfg(windows)]
mod windows_screens {
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplayDevicesW, DISPLAY_DEVICEW};
    use windows_sys::Win32::UI::WindowsAndMessaging::EDD_GET_DEVICE_INTERFACE_NAME;

    /// The monitor shown on the display `display` (`\\.\DISPLAY2`): its device's interface,
    /// which holds its model and the port it is on (`\\?\DISPLAY#GSM5B7F#5&…&UID4353#{…}`).
    pub fn device_of(display: &str) -> Option<String> {
        let display: Vec<u16> = display.encode_utf16().chain(Some(0)).collect();
        // SAFETY: the name is terminated and outlives the call, and the structure is the size
        // it says it is
        let device = unsafe {
            let mut device: DISPLAY_DEVICEW = std::mem::zeroed();
            device.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
            (EnumDisplayDevicesW(display.as_ptr(), 0, &mut device, EDD_GET_DEVICE_INTERFACE_NAME) != 0)
                .then_some(device)?
        };
        let id = &device.DeviceID;
        let end = id.iter().position(|&c| c == 0).unwrap_or(id.len());
        (end > 0).then(|| String::from_utf16_lossy(&id[..end]))
    }
}
