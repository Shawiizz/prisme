//! Prisme's new versions on a computer, as the programs of today bring theirs: looked for a
//! moment after Prisme starts and every few hours, downloaded in the background (or when the
//! DJ asks, automatic updates off), checked against the releases' key, then put in place of the
//! running one (`prisme_update`). The next launch is the new version, without an installer; the
//! DJ may restart into it at once, the decks stopped. A copy the system's packages installed
//! (a .deb) is only told of it; Android has its APK (`commands::apk`).

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use prisme_update::{Identity, Install, Manifest};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

/// What the page hears when the state of the updates changes.
pub const UPDATE_EVENT: &str = "update-state";
/// The first look, once Prisme settled after its start.
const FIRST_LOOK: Duration = Duration::from_secs(10);
/// The looks after it: a long night is a few of them.
const LOOK_EVERY: Duration = Duration::from_secs(4 * 3600);
/// A silent server is given up after this long.
const TIMEOUT: Duration = Duration::from_secs(30);
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where the new versions stand, as the page shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "stage", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "UpdateState"))]
pub enum UpdateState {
    /// Not looked for yet.
    Unknown,
    Looking,
    /// The version running is the latest.
    Latest,
    /// The releases could not be reached.
    Unreachable,
    /// A newer version, not downloaded: when the DJ asks (automatic updates off), else by the
    /// system's packages or by hand (`installsItself` false).
    Available {
        version: String,
        installs_itself: bool,
    },
    Downloading {
        version: String,
        progress: f64,
    },
    /// In place: the next launch is it.
    Ready {
        version: String,
    },
    Failed {
        version: String,
        error: String,
    },
}

/// What the DJ asks of the updates' thread.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Ask {
    /// Look for a new version, saying how it went.
    Look,
    Download,
}

pub struct Updates {
    #[cfg(windows)]
    identity: Identity,
    install: Install,
    state: Mutex<UpdateState>,
    asks: Mutex<Option<Sender<Ask>>>,
    /// The DJ asked to restart into the new version: started as Prisme quits.
    restart: AtomicBool,
}

impl Updates {
    pub fn new() -> Self {
        let identity = Identity::current();
        let install = Install::of_this_process(&identity);
        Self {
            #[cfg(windows)]
            identity,
            install,
            state: Mutex::new(UpdateState::Unknown),
            asks: Mutex::new(None),
            restart: AtomicBool::new(false),
        }
    }

    fn state(&self) -> UpdateState {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn set(&self, app: &AppHandle, state: UpdateState) {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = state.clone();
        let _ = app.emit(UPDATE_EVENT, state);
    }

    /// Looks for a new version now: one found is downloaded, automatic updates on.
    pub fn look(&self) -> AppResult<()> {
        self.ask(Ask::Look)
    }

    fn ask(&self, ask: Ask) -> AppResult<()> {
        let asks = self.asks.lock().unwrap_or_else(PoisonError::into_inner);
        let sent = asks.as_ref().is_some_and(|asks| asks.send(ask).is_ok());
        sent.then_some(()).ok_or_else(|| AppError::Other("the updates are not looked for".into()))
    }

    /// As Prisme quits: the new version set beside the running one put in place (macOS), and
    /// started if the DJ asked to restart into it.
    pub fn on_quit(&self) {
        if let Err(e) = self.install.apply(VERSION) {
            tracing::warn!("the new version waits for the next start: {e}");
        }
        if self.restart.load(Ordering::Acquire) {
            if let Err(e) = self.install.launch_after(std::process::id()) {
                tracing::error!("the new version did not start: {e}");
            }
        }
    }
}

/// What to do before Prisme starts, as installed: its arguments' asks (its uninstallation on
/// Windows), the exit code to end with if one ends it; a new version that waited put in place
/// and started instead; what earlier updates and the NSIS installer of the versions before 0.7
/// left cleared.
pub fn before_start() -> Option<i32> {
    let identity = Identity::current();
    if let Some(code) = prisme_update::on_launch(&identity) {
        return Some(code);
    }
    let install = Install::of_this_process(&identity);
    match install.apply(VERSION) {
        Ok(Some(_)) if install.launch_after(std::process::id()).is_ok() => return Some(0),
        Err(e) => eprintln!("the new version waits: {e}"),
        _ => {}
    }
    install.tidy();
    #[cfg(windows)]
    if let Install::Windows { exe } = &install {
        use prisme_update::windows::{clear_updater_leftovers, forget_nsis, register, shortcuts};
        // installed by NSIS before 0.7: the entry and the shortcuts become Prisme's own
        if exe.parent().is_some_and(|folder| forget_nsis(&identity, folder)) {
            let _ = register(&identity, exe, VERSION);
            let _ = shortcuts(&identity, exe, false);
        }
        clear_updater_leftovers(&identity, Duration::from_secs(60));
    }
    None
}

/// Starts looking for new versions, on a thread of its own at a low priority.
pub fn start(app: &AppHandle) {
    let (sender, asks) = mpsc::channel();
    *app.state::<Updates>().asks.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("prisme-updates".into()).spawn(move || {
        prisme_library::background_priority();
        run(&app, &asks);
    });
    if let Err(e) = spawned {
        tracing::warn!("new versions are not looked for: {e}");
    }
}

fn run(app: &AppHandle, asks: &Receiver<Ask>) {
    let updates = app.state::<Updates>();
    let mut next_look = Instant::now() + FIRST_LOOK;
    // the manifest of the last look, for a download asked after it
    let mut latest: Option<Manifest> = None;
    loop {
        let ask = match asks.recv_timeout(next_look.saturating_duration_since(Instant::now())) {
            Ok(ask) => Some(ask),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if ask.is_none() {
            next_look = Instant::now() + LOOK_EVERY;
        }
        // in place already: the next launch is it
        if matches!(updates.state(), UpdateState::Ready { .. }) {
            continue;
        }
        match ask {
            None | Some(Ask::Look) => {
                if ask.is_some() {
                    updates.set(app, UpdateState::Looking);
                }
                match fetch_manifest() {
                    Ok(manifest) => {
                        let automatic = app.state::<AppState>().preferences.get().updates.automatic;
                        let downloads = automatic && updates.install.platform().is_some();
                        let newer = prisme_update::newer(&manifest.version, VERSION);
                        latest = Some(manifest.clone());
                        if newer && downloads {
                            download(app, &updates, &manifest);
                        } else if newer {
                            let installs_itself = updates.install.platform().is_some();
                            updates.set(
                                app,
                                UpdateState::Available { version: manifest.version, installs_itself },
                            );
                        } else {
                            updates.set(app, UpdateState::Latest);
                        }
                    }
                    Err(e) => {
                        tracing::info!("no new version looked for: {e}");
                        if ask.is_some() || updates.state() == UpdateState::Looking {
                            updates.set(app, UpdateState::Unreachable);
                        }
                    }
                }
            }
            Some(Ask::Download) => match &latest {
                Some(manifest) if prisme_update::newer(&manifest.version, VERSION) => {
                    download(app, &updates, manifest)
                }
                _ => updates.set(app, UpdateState::Latest),
            },
        }
    }
}

fn fetch_manifest() -> Result<Manifest, String> {
    let client = crate::http::client(TIMEOUT)?;
    let bytes = tauri::async_runtime::block_on(crate::http::fetch(&client, &prisme_update::manifest_url()))?;
    Manifest::parse(&bytes).map_err(|e| e.to_string())
}

/// Downloads the package of `manifest`'s version, checks it and puts it in place.
fn download(app: &AppHandle, updates: &Updates, manifest: &Manifest) {
    let version = manifest.version.clone();
    let put = || -> Result<(), String> {
        let platform = updates.install.platform().ok_or("this copy does not update itself")?;
        let package = manifest.package(platform).ok_or_else(|| format!("no package for {platform}"))?;
        updates.set(app, UpdateState::Downloading { version: version.clone(), progress: 0.0 });
        let told =
            |progress| updates.set(app, UpdateState::Downloading { version: version.clone(), progress });
        let bytes = tauri::async_runtime::block_on(fetch_told(&package.url, told))?;
        prisme_update::verify(&bytes, &package.signature, &version).map_err(|e| e.to_string())?;
        updates.install.stage(&version, &bytes).map_err(|e| e.to_string())?;
        #[cfg(windows)]
        if let Install::Windows { exe } = &updates.install {
            let _ = prisme_update::windows::register(&updates.identity, exe, &version);
        }
        Ok(())
    };
    match put() {
        Ok(()) => {
            tracing::info!("Prisme {version} is in place for the next start");
            updates.set(app, UpdateState::Ready { version });
        }
        Err(error) => {
            tracing::warn!("Prisme {version} was not put in place: {error}");
            updates.set(app, UpdateState::Failed { version, error });
        }
    }
}

/// The body of `url`, telling `told` how much of it came (0 to 1), a hundredth at a time.
async fn fetch_told(url: &str, told: impl Fn(f64)) -> Result<Vec<u8>, String> {
    let client = crate::http::client(TIMEOUT)?;
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    let mut response = response.error_for_status().map_err(|e| e.to_string())?;
    let total = response.content_length().filter(|&n| n > 0);
    let mut bytes = Vec::with_capacity(total.unwrap_or(0) as usize);
    let mut shown = 0.0;
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        bytes.extend_from_slice(&chunk);
        let progress = total.map_or(0.0, |total| bytes.len() as f64 / total as f64);
        if progress - shown >= 0.01 {
            shown = progress;
            told(progress);
        }
    }
    Ok(bytes)
}

#[tauri::command]
pub fn update_state(updates: State<'_, Updates>) -> UpdateState {
    updates.state()
}

/// Looks for a new version now.
#[tauri::command]
pub fn look_for_update(updates: State<'_, Updates>) -> AppResult<()> {
    updates.look()
}

/// Downloads the new version found, automatic updates off.
#[tauri::command]
pub fn download_update(updates: State<'_, Updates>) -> AppResult<()> {
    updates.ask(Ask::Download)
}

/// Quits Prisme and starts the new version in place, once the decks are stopped.
#[tauri::command]
pub fn restart_to_update(
    app: AppHandle,
    state: State<'_, AppState>,
    updates: State<'_, Updates>,
) -> AppResult<()> {
    crate::commands::new_version::check_decks_stopped(&state)?;
    if !matches!(updates.state(), UpdateState::Ready { .. }) {
        return Err(AppError::Other("no new version in place".into()));
    }
    updates.restart.store(true, Ordering::Release);
    app.exit(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_is_told_the_stage_and_what_goes_with_it() {
        let ready = serde_json::to_value(UpdateState::Ready { version: "0.7.0".into() }).unwrap();
        assert_eq!(ready, serde_json::json!({ "stage": "ready", "version": "0.7.0" }));
        let available = UpdateState::Available { version: "0.7.0".into(), installs_itself: false };
        let available = serde_json::to_value(available).unwrap();
        assert_eq!(available["installsItself"], false);
        assert_eq!(
            serde_json::to_value(UpdateState::Unknown).unwrap(),
            serde_json::json!({ "stage": "unknown" })
        );
    }
}
