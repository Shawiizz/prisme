//! The installation: the window shows, the NSIS installer is written to the temporary folder and
//! run silently (`/S`) while the bar fills, then Prisme opens from where it was installed and the
//! window fades out. Without the WebView2 runtime to show the window, the NSIS installer runs
//! with its own.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::{WindowBuilderExtWindows, WindowExtWindows};
use tao::window::{Window, WindowBuilder};
use windows_sys::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use wry::{WebContext, WebView, WebViewBuilder};

/// The NSIS installer of this version (empty in a development build).
const PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.exe"));
const PAGE: &str = include_str!(concat!(env!("OUT_DIR"), "/page.html"));
const SIZE: LogicalSize<f64> = LogicalSize::new(440.0, 280.0);
/// The executable an installed Prisme leaves running, which the installation cannot replace.
const APP_EXE: &str = "prisme-desktop.exe";
/// Where the NSIS installer says where it put Prisme.
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Prisme";
/// The bar nears its end as an installation of this many seconds would.
const TYPICAL_S: f64 = 4.0;
const TICK: Duration = Duration::from_millis(50);
/// Installed, the window stays this long before Prisme opens, then fades out for this long.
const DONE_SHOWN: Duration = Duration::from_millis(1100);
const FADE: Duration = Duration::from_millis(280);
const NO_WINDOW: u32 = 0x0800_0000;

/// What happens, as the window's loop hears it.
#[derive(Debug)]
enum Happened {
    /// The page is ready to be told.
    Ready,
    /// The installation went this far (0 to 1).
    Progress(f64),
    Installed,
    Failed(String),
    /// The page asks: move the window, close it, install again.
    Drag,
    Close,
    Retry,
    /// Prisme opened: the window goes.
    Leave,
}

pub fn run() {
    let texts = Texts::of_the_system();
    let event_loop = EventLoopBuilder::<Happened>::with_user_event().build();
    let window = WindowBuilder::new()
        .with_title("Prisme")
        .with_inner_size(SIZE)
        .with_resizable(false)
        .with_decorations(false)
        .with_undecorated_shadow(true)
        // over the other windows for the few seconds it lasts: Windows may refuse to bring a
        // window forward when it opens, and it would open behind the one in use
        .with_always_on_top(true)
        .with_visible(false)
        .build(&event_loop)
        .expect("the installer's window");
    centre(&window);
    round_corners(&window);
    let proxy = event_loop.create_proxy();
    let asks = proxy.clone();
    let init = format!("window.TEXTS = {};", texts.json());
    // the web view keeps its files in the temporary folder, not beside the installer
    let mut context = WebContext::new(Some(std::env::temp_dir().join("Prisme setup")));
    let webview = WebViewBuilder::new_with_web_context(&mut context)
        .with_html(PAGE)
        .with_initialization_script(init)
        .with_background_color((10, 11, 13, 255))
        .with_ipc_handler(move |request| {
            let asked = match request.body().as_str() {
                "ready" => Happened::Ready,
                "drag" => Happened::Drag,
                "close" => Happened::Close,
                "retry" => Happened::Retry,
                _ => return,
            };
            let _ = asks.send_event(asked);
        })
        .build(&window);
    let Ok(webview) = webview else {
        // no web view to show the window with: the installer shows its own
        return install_with_its_own_window();
    };

    let mut started = false;
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(Happened::Ready) if !started => {
                started = true;
                window.set_visible(true);
                window.set_focus();
                install_in_the_background(proxy.clone());
            }
            Event::UserEvent(Happened::Progress(share)) => call(&webview, &format!("progress({share})")),
            Event::UserEvent(Happened::Installed) => {
                call(&webview, "done()");
                let proxy = proxy.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(DONE_SHOWN);
                    open_prisme();
                    let _ = proxy.send_event(Happened::Leave);
                });
            }
            Event::UserEvent(Happened::Failed(message)) => {
                call(&webview, &format!("failed({})", serde_json::Value::String(message)));
            }
            Event::UserEvent(Happened::Retry) => install_in_the_background(proxy.clone()),
            Event::UserEvent(Happened::Drag) => {
                let _ = window.drag_window();
            }
            Event::UserEvent(Happened::Leave) => {
                call(&webview, "leave()");
                let proxy = proxy.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(FADE);
                    let _ = proxy.send_event(Happened::Close);
                });
            }
            Event::UserEvent(Happened::Close)
            | Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });
}

fn call(webview: &WebView, method: &str) {
    let _ = webview.evaluate_script(&format!("window.setup.{method}"));
}

/// In the middle of the screen the window opens on.
fn centre(window: &Window) {
    let Some(monitor) = window.current_monitor() else { return };
    let (screen, at, size) = (monitor.size(), monitor.position(), window.outer_size());
    let x = at.x + (screen.width as i32 - size.width as i32) / 2;
    let y = at.y + (screen.height as i32 - size.height as i32) / 2;
    window.set_outer_position(PhysicalPosition::new(x, y));
}

/// Windows 11 rounds the window as its own; earlier ones leave it square.
fn round_corners(window: &Window) {
    let round = DWMWCP_ROUND;
    // SAFETY: the window handle is alive, the attribute a 4-byte value read during the call
    unsafe {
        DwmSetWindowAttribute(
            window.hwnd() as _,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            (&raw const round).cast(),
            size_of_val(&round) as u32,
        );
    }
}

/// Installs on a thread of its own, telling the window how far it went.
fn install_in_the_background(proxy: EventLoopProxy<Happened>) {
    std::thread::spawn(move || {
        let outcome = install(&proxy);
        let _ = proxy.send_event(match outcome {
            Ok(()) => Happened::Installed,
            Err(message) => Happened::Failed(message),
        });
    });
}

fn install(proxy: &EventLoopProxy<Happened>) -> Result<(), String> {
    let texts = Texts::of_the_system();
    let _ = proxy.send_event(Happened::Progress(0.0));
    if prisme_is_running() {
        return Err(texts.close_prisme.into());
    }
    let started = Instant::now();
    let progress = || {
        // nearing the end as a typical installation would, never reaching it before it is done
        let share = 0.95 * (1.0 - (-started.elapsed().as_secs_f64() / TYPICAL_S).exp());
        let _ = proxy.send_event(Happened::Progress(share));
    };
    if PAYLOAD.is_empty() {
        // a development build carries no installer: the installation is only played
        while started.elapsed() < Duration::from_secs_f64(TYPICAL_S) {
            progress();
            std::thread::sleep(TICK);
        }
        return Ok(());
    }
    let installer = installer_path();
    std::fs::write(&installer, PAYLOAD).map_err(|e| format!("{}: {e}", texts.not_written))?;
    let mut child = Command::new(&installer)
        .arg("/S")
        .creation_flags(NO_WINDOW)
        .spawn()
        .map_err(|e| format!("{}: {e}", texts.not_started))?;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        progress();
        std::thread::sleep(TICK);
    };
    let _ = std::fs::remove_file(&installer);
    if status.success() {
        Ok(())
    } else {
        Err(texts.failed.replace("{code}", &status.code().unwrap_or(-1).to_string()))
    }
}

/// The NSIS installer, the window shown by itself: what runs without a web view.
fn install_with_its_own_window() {
    let installer = installer_path();
    if std::fs::write(&installer, PAYLOAD).is_ok() {
        let _ = Command::new(&installer).status();
        let _ = std::fs::remove_file(&installer);
    }
}

fn installer_path() -> PathBuf {
    std::env::temp_dir().join(format!("Prisme {} setup.exe", env!("CARGO_PKG_VERSION")))
}

fn prisme_is_running() -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("IMAGENAME eq {APP_EXE}"), "/NH"])
        .creation_flags(NO_WINDOW)
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).to_lowercase().contains(APP_EXE))
}

/// Opens Prisme where the installer put it (none was, in a development build).
fn open_prisme() {
    if PAYLOAD.is_empty() {
        return;
    }
    let Some(folder) = registry_text(UNINSTALL_KEY, "InstallLocation") else { return };
    let exe = registry_text(UNINSTALL_KEY, "MainBinaryName").unwrap_or_else(|| APP_EXE.into());
    let folder = PathBuf::from(folder.trim_matches('"'));
    let _ = Command::new(folder.join(exe)).current_dir(&folder).spawn();
}

/// A text value of the current user's registry.
fn registry_text(key: &str, name: &str) -> Option<String> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (key, name) = (wide(key), wide(name));
    let mut buffer = vec![0u16; 1024];
    let mut bytes = (buffer.len() * 2) as u32;
    // SAFETY: the strings end with a nul, the buffer holds `bytes` bytes
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status != 0 {
        return None;
    }
    let chars = (bytes as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buffer[..chars]))
}

/// What the window says, in French on a French system, else in English.
struct Texts {
    installing: &'static str,
    installed: &'static str,
    opening: &'static str,
    close: &'static str,
    retry: &'static str,
    close_prisme: &'static str,
    not_written: &'static str,
    not_started: &'static str,
    failed: &'static str,
}

impl Texts {
    fn of_the_system() -> Self {
        // SAFETY: no argument, no state
        let language = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
        const FRENCH: u16 = 0x0c;
        if language & 0x3ff == FRENCH {
            Self {
                installing: "Installation de Prisme…",
                installed: "Prisme est installé",
                opening: "Ouverture…",
                close: "Fermer",
                retry: "Réessayer",
                close_prisme: "Prisme est ouvert : fermez-le pour l’installer.",
                not_written: "L’installation n’a pas pu être préparée",
                not_started: "L’installation n’a pas pu démarrer",
                failed: "L’installation n’a pas abouti (code {code}).",
            }
        } else {
            Self {
                installing: "Installing Prisme…",
                installed: "Prisme is installed",
                opening: "Opening…",
                close: "Close",
                retry: "Try again",
                close_prisme: "Prisme is open: close it to install.",
                not_written: "The installation could not be prepared",
                not_started: "The installation could not start",
                failed: "The installation did not finish (code {code}).",
            }
        }
    }

    /// What the page shows by itself.
    fn json(&self) -> String {
        serde_json::json!({
            "installing": self.installing,
            "installed": self.installed,
            "opening": self.opening,
            "close": self.close,
            "retry": self.retry,
        })
        .to_string()
    }
}
