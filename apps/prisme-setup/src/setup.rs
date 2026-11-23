//! The installation: the window shows, Prisme is unpacked into its folder of the user's local
//! data while the bar fills, its shortcuts and its entry among the installed applications are
//! written, then Prisme opens and the window fades out. No administrator is asked for. The
//! versions before 0.7 run it as their update (`/UPDATE`), from the NSIS installer they had:
//! what NSIS left is cleared. Without the WebView2 runtime to show the window (and Prisme),
//! Microsoft's installer of it runs first.

use prisme_update::windows::{self as system, french, runs_from};
use prisme_update::{Identity, Install, EXE};
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

/// Prisme's package of this version (empty in a development build).
const PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.tar.gz"));
/// Microsoft's installer of the WebView2 runtime (empty in a development build).
const WEBVIEW2: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/webview2.exe"));
const VERSION: &str = env!("CARGO_PKG_VERSION");
const PAGE: &str = include_str!(concat!(env!("OUT_DIR"), "/page.html"));
const SIZE: LogicalSize<f64> = LogicalSize::new(440.0, 280.0);
/// The bar nears its end as an installation of this many seconds would, and shows that long
/// at least, to be read.
const TYPICAL_S: f64 = 2.0;
/// How long the version updated from is waited for to quit.
const QUITTING: Duration = Duration::from_secs(20);
const TICK: Duration = Duration::from_millis(50);
/// Installed, the window stays this long before Prisme opens, then fades out for this long.
const DONE_SHOWN: Duration = Duration::from_millis(1100);
const FADE: Duration = Duration::from_millis(280);

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

/// How it was started: by the DJ, or by Tauri's updater of a version before 0.7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Fresh,
    Update,
}

impl Mode {
    fn of_the_arguments() -> Self {
        if std::env::args().skip(1).any(|a| a.eq_ignore_ascii_case("/UPDATE")) {
            Self::Update
        } else {
            Self::Fresh
        }
    }
}

pub fn run() {
    let texts = Texts::of_the_system();
    let mode = Mode::of_the_arguments();
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
    let init = format!("window.TEXTS = {};", texts.json());
    // the web view keeps its files in the temporary folder, not beside the installer
    let mut context = WebContext::new(Some(std::env::temp_dir().join("Prisme setup")));
    let build = |context: &mut WebContext| {
        let asks = proxy.clone();
        WebViewBuilder::new_with_web_context(context)
            .with_html(PAGE)
            .with_initialization_script(&init)
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
            .build(&window)
    };
    // no runtime to show the page with: Microsoft's installer of it first, which Prisme needs too
    let webview = build(&mut context).or_else(|_| {
        install_webview2();
        build(&mut context)
    });
    let Ok(webview) = webview else {
        system::message(texts.no_webview);
        return;
    };

    let mut started = false;
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(Happened::Ready) if !started => {
                started = true;
                window.set_visible(true);
                window.set_focus();
                install_in_the_background(proxy.clone(), mode);
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
            Event::UserEvent(Happened::Retry) => install_in_the_background(proxy.clone(), mode),
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
fn install_in_the_background(proxy: EventLoopProxy<Happened>, mode: Mode) {
    std::thread::spawn(move || {
        let outcome = install(&proxy, mode);
        let _ = proxy.send_event(match outcome {
            Ok(()) => Happened::Installed,
            Err(message) => Happened::Failed(message),
        });
    });
}

fn install(proxy: &EventLoopProxy<Happened>, mode: Mode) -> Result<(), String> {
    let texts = Texts::of_the_system();
    let started = Instant::now();
    let progress = || {
        // nearing the end as a typical installation would, never reaching it before it is done
        let share = 0.95 * (1.0 - (-started.elapsed().as_secs_f64() / TYPICAL_S).exp());
        let _ = proxy.send_event(Happened::Progress(share));
    };
    progress();
    let folder = Identity::current().folder().ok_or("no local data folder")?;
    // the version updated from quits as it starts the installer
    while mode == Mode::Update && runs_from(&folder) && started.elapsed() < QUITTING {
        progress();
        std::thread::sleep(TICK);
    }
    if runs_from(&folder) {
        return Err(texts.close_prisme.into());
    }
    let installed = std::thread::scope(|scope| {
        let installing = scope.spawn(|| put_in_place(mode));
        while !installing.is_finished() || started.elapsed().as_secs_f64() < TYPICAL_S {
            progress();
            std::thread::sleep(TICK);
        }
        installing.join().unwrap_or_else(|_| Err("stopped".into()))
    });
    installed.map_err(|e| format!("{} ({e}).", texts.failed))
}

/// Prisme unpacked into its folder, in the place of the version there if one is, then its
/// shortcuts and its entry among the installed applications; what NSIS left cleared. Nothing in
/// a development build, which carries no Prisme.
fn put_in_place(mode: Mode) -> Result<(), String> {
    if PAYLOAD.is_empty() {
        return Ok(());
    }
    let identity = Identity::current();
    let folder = identity.folder().ok_or("no local data folder")?;
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let exe = folder.join(EXE);
    let install = Install::Windows { exe: exe.clone() };
    install.stage(VERSION, PAYLOAD).map_err(|e| e.to_string())?;
    install.tidy();
    system::forget_nsis(&identity, &folder);
    system::register(&identity, &exe, VERSION).map_err(|e| e.to_string())?;
    system::shortcuts(&identity, &exe, mode == Mode::Fresh).map_err(|e| e.to_string())
}

/// Microsoft's installer of the WebView2 runtime, run with its own window until it is done.
fn install_webview2() {
    if WEBVIEW2.is_empty() {
        return;
    }
    let installer = std::env::temp_dir().join("MicrosoftEdgeWebview2Setup.exe");
    if std::fs::write(&installer, WEBVIEW2).is_ok() {
        let _ = Command::new(&installer).arg("/install").status();
        let _ = std::fs::remove_file(&installer);
    }
}

/// Opens Prisme where it was installed (none was, in a development build).
fn open_prisme() {
    if PAYLOAD.is_empty() {
        return;
    }
    let Some(folder) = Identity::current().folder() else { return };
    let _ = Command::new(folder.join(EXE)).current_dir(&folder).spawn();
}

/// What the window says, in French on a French system, else in English.
struct Texts {
    installing: &'static str,
    installed: &'static str,
    opening: &'static str,
    close: &'static str,
    retry: &'static str,
    close_prisme: &'static str,
    failed: &'static str,
    no_webview: &'static str,
}

impl Texts {
    fn of_the_system() -> Self {
        if french() {
            Self {
                installing: "Installation de Prisme…",
                installed: "Prisme est installé",
                opening: "Ouverture…",
                close: "Fermer",
                retry: "Réessayer",
                close_prisme: "Prisme est ouvert : fermez-le pour l’installer.",
                failed: "L’installation n’a pas abouti",
                no_webview: "Prisme a besoin du runtime WebView2 de Microsoft, qui n’a pas pu être installé. Installez-le depuis le site de Microsoft, puis relancez l’installation.",
            }
        } else {
            Self {
                installing: "Installing Prisme…",
                installed: "Prisme is installed",
                opening: "Opening…",
                close: "Close",
                retry: "Try again",
                close_prisme: "Prisme is open: close it to install.",
                failed: "The installation did not finish",
                no_webview: "Prisme needs Microsoft's WebView2 runtime, which could not be installed. Install it from Microsoft's site, then run the installation again.",
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
