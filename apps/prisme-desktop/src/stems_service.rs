//! Vocals separation in the background: a queue of tracks worked through on one thread,
//! on the GPU when there is a usable one, and the model it needs, downloaded once.
//!
//! The separations are stored next to the tracks (`prisme_stems::track`): a track loaded
//! on a deck gets its vocals as soon as they exist.

use crate::dto::{DeckTrackUpdateDto, ModelStatusDto, StemsProgressDto, StemsStatusDto};
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState};
use crate::track_loading::DECK_TRACK_UPDATE_EVENT;
use prisme_engine::Command;
use prisme_stems::{CpuNetwork, GpuNetwork, MaskNetwork, Separator, StemsError, Weights};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};
use tracing::{error, info, warn};

pub const STEMS_PROGRESS_EVENT: &str = "stems-progress";
/// Payload: the path of the track whose vocals were just stored.
pub const STEMS_SEPARATED_EVENT: &str = "stems-separated";
pub const STEMS_MODEL_EVENT: &str = "stems-model";

/// The Mel-Band RoFormer vocals model of Kimberley Jensen (GPL-3.0), converted by
/// `crates/prisme-stems/tools/export_roformer.py --half`.
const MODEL_FILE: &str = "roformer-vocals-f16-1.safetensors";
const MODEL_URL: &str =
    "https://github.com/Shawiizz/prisme/releases/download/model-roformer-vocals-1/roformer-vocals-f16-1.safetensors";
const MODEL_SHA256: &str = "90397548e9e427b2645ba33a71f5e3de3f1e01997f7c28fc4b0914fee7ba21ee";
const MODEL_BYTES: u64 = 456_488_724;

pub struct StemsService {
    app: AppHandle,
    model: PathBuf,
    queue: Arc<Mutex<Queue>>,
    cancel: Arc<AtomicBool>,
    download: Arc<Mutex<Download>>,
}

#[derive(Default)]
struct Queue {
    waiting: VecDeque<PathBuf>,
    progress: StemsProgressDto,
}

#[derive(Default)]
struct Download {
    running: bool,
    fraction: f64,
    error: Option<String>,
}

impl StemsService {
    pub fn new(app: AppHandle, data_dir: Option<&Path>) -> Self {
        let model = data_dir.unwrap_or(Path::new(".")).join("models").join(MODEL_FILE);
        Self { app, model, queue: Arc::default(), cancel: Arc::default(), download: Arc::default() }
    }

    pub fn status(&self) -> StemsStatusDto {
        StemsStatusDto { model: self.model_status(), progress: lock(&self.queue).progress.clone() }
    }

    fn model_status(&self) -> ModelStatusDto {
        let download = lock(&self.download);
        ModelStatusDto {
            installed: self.model.is_file(),
            downloading: download.running.then_some(download.fraction),
            error: download.error.clone(),
            bytes: MODEL_BYTES,
        }
    }

    /// Queues the tracks that have no vocals yet; starts working if idle.
    pub fn enqueue(&self, paths: impl IntoIterator<Item = PathBuf>) -> AppResult<usize> {
        if !self.model.is_file() {
            return Err(AppError::NoStemsModel);
        }
        let mut queue = lock(&self.queue);
        let before = queue.waiting.len();
        let current = queue.progress.current.clone();
        for path in paths {
            let already = queue.waiting.contains(&path)
                || current.as_deref() == Some(&*path.to_string_lossy())
                || prisme_stems::track::has_vocals(&path);
            if !already {
                queue.waiting.push_back(path);
            }
        }
        let added = queue.waiting.len() - before;
        queue.progress.queued = queue.waiting.len() + usize::from(queue.progress.running);
        if added > 0 && !queue.progress.running {
            queue.progress =
                StemsProgressDto { running: true, queued: queue.waiting.len(), ..Default::default() };
            self.cancel.store(false, Ordering::Relaxed);
            let worker = Worker {
                app: self.app.clone(),
                model: self.model.clone(),
                queue: self.queue.clone(),
                cancel: self.cancel.clone(),
            };
            std::thread::Builder::new()
                .name("prisme-stems".into())
                .spawn(move || worker.run())
                .map_err(|e| AppError::Other(e.to_string()))?;
        }
        let _ = self.app.emit(STEMS_PROGRESS_EVENT, queue.progress.clone());
        Ok(added)
    }

    /// Empties the queue and stops the track being separated.
    pub fn cancel(&self) {
        lock(&self.queue).waiting.clear();
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Downloads the model in the background (`stems-model` events); nothing if it is
    /// there or on its way.
    pub fn download_model(&self) {
        {
            let mut download = lock(&self.download);
            if download.running || self.model.is_file() {
                return;
            }
            *download = Download { running: true, ..Default::default() };
        }
        let (app, target, download) = (self.app.clone(), self.model.clone(), self.download.clone());
        tauri::async_runtime::spawn(async move {
            let result = fetch_model(&app, &target, &download).await;
            let mut state = lock(&download);
            state.running = false;
            if let Err(e) = result {
                error!("model download failed: {e}");
                state.error = Some(e);
            }
            drop(state);
            let _ = app.emit(STEMS_MODEL_EVENT, app.state::<StemsService>().model_status());
        });
    }
}

async fn fetch_model(app: &AppHandle, target: &Path, download: &Mutex<Download>) -> Result<(), String> {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let folder = target.parent().ok_or("no folder for the model")?;
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let partial = target.with_extension("part");
    let client = reqwest::Client::builder().user_agent("Prisme").build().map_err(|e| e.to_string())?;
    let mut response =
        client.get(MODEL_URL).send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
    let mut file = std::fs::File::create(&partial).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut received = 0u64;
    let mut reported = 0.0;
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        hash.update(&chunk);
        received += chunk.len() as u64;
        let fraction = received as f64 / MODEL_BYTES as f64;
        if fraction - reported >= 0.005 {
            reported = fraction;
            lock(download).fraction = fraction;
            let _ = app.emit(STEMS_MODEL_EVENT, app.state::<StemsService>().model_status());
        }
    }
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if received != MODEL_BYTES || digest != MODEL_SHA256 {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("the downloaded model is damaged ({received} bytes)"));
    }
    std::fs::rename(&partial, target).map_err(|e| e.to_string())?;
    info!("separation model installed in {}", target.display());
    Ok(())
}

struct Worker {
    app: AppHandle,
    model: PathBuf,
    queue: Arc<Mutex<Queue>>,
    cancel: Arc<AtomicBool>,
}

impl Worker {
    fn run(self) {
        prisme_library::background_priority();
        let mut separator = None;
        while let Some(path) = self.next() {
            if separator.is_none() {
                match self.open() {
                    Ok((s, device)) => {
                        self.update(|p| p.device = Some(device));
                        separator = Some(s);
                    }
                    Err(e) => {
                        error!("separation unavailable: {e}");
                        let dropped = std::mem::take(&mut lock(&self.queue).waiting).len();
                        self.update(|p| p.failed += 1 + dropped);
                        break;
                    }
                }
            }
            let separator = separator.as_mut().expect("opened above");
            let result = prisme_stems::track::separate_file(separator, &path, |done, total| {
                self.update(|p| p.fraction = done as f64 / total as f64);
                !self.cancel.load(Ordering::Relaxed)
            });
            match result {
                Ok(_) => {
                    self.update(|p| p.done += 1);
                    let _ = self.app.emit(STEMS_SEPARATED_EVENT, path.display().to_string());
                    attach_vocals(&self.app, &path);
                }
                Err(StemsError::Cancelled) => info!("separation of {} cancelled", path.display()),
                Err(e) => {
                    warn!("{} not separated: {e}", path.display());
                    self.update(|p| p.failed += 1);
                }
            }
        }
        // the separator (and the GPU memory it holds) goes with the thread
        self.update(|p| {
            p.running = false;
            p.current = None;
            p.queued = 0;
        });
    }

    fn next(&self) -> Option<PathBuf> {
        let mut queue = lock(&self.queue);
        let path = queue.waiting.pop_front();
        queue.progress.current = path.as_ref().map(|p| p.display().to_string());
        queue.progress.fraction = 0.0;
        queue.progress.queued = queue.waiting.len() + usize::from(path.is_some());
        drop(queue);
        self.update(|_| {});
        path
    }

    fn open(&self) -> Result<(Separator<Box<dyn MaskNetwork>>, String), StemsError> {
        let weights = Weights::load(&self.model)?;
        let (network, device): (Box<dyn MaskNetwork>, String) = match GpuNetwork::new(weights.clone()) {
            Ok(gpu) => {
                let device = gpu.adapter().to_owned();
                (Box::new(gpu), device)
            }
            Err(e) => {
                warn!("separating on the CPU: {e}");
                (Box::new(CpuNetwork::new(weights)), "CPU".to_owned())
            }
        };
        info!("separating with {device}");
        Ok((Separator::new(network), device))
    }

    fn update(&self, f: impl FnOnce(&mut StemsProgressDto)) {
        let progress = {
            let mut queue = lock(&self.queue);
            f(&mut queue.progress);
            queue.progress.clone()
        };
        let _ = self.app.emit(STEMS_PROGRESS_EVENT, progress);
    }
}

/// Gives the stored vocals of `path` to the decks it is loaded on (once fully decoded:
/// otherwise the end of its loading does it).
pub fn attach_vocals(app: &AppHandle, path: &Path) {
    let state = app.state::<AppState>();
    let loaded: Vec<(usize, u64, Arc<prisme_media::TrackAudio>)> = lock(&state.tracks)
        .iter()
        .enumerate()
        .filter_map(|(deck, t)| {
            t.as_ref().filter(|t| t.path == path).map(|t| (deck, t.load_id, t.audio.clone()))
        })
        .collect();
    for (deck, load_id, audio) in loaded {
        if audio.is_complete() && give_vocals(app, deck, load_id, path, &audio) {
            let update = DeckTrackUpdateDto {
                deck,
                load_id,
                waveform_ready: false,
                analysis: None,
                vocals_ready: true,
            };
            let _ = app.emit(DECK_TRACK_UPDATE_EVENT, update);
        }
    }
}

/// Loads the stored vocals of the track loaded on `deck` as `load_id` and sends them to the
/// engine; `true` if it got some (the caller tells the UI).
pub fn give_vocals(
    app: &AppHandle,
    deck: usize,
    load_id: u64,
    path: &Path,
    audio: &prisme_media::TrackAudio,
) -> bool {
    let vocals = match prisme_stems::track::load_vocals(path, audio.frames(), audio.sample_rate()) {
        Ok(Some(vocals)) => Arc::new(vocals),
        Ok(None) => return false,
        Err(e) => {
            warn!("vocals of {} not read: {e}", path.display());
            return false;
        }
    };
    let state = app.state::<AppState>();
    let given = state.with_loaded(deck, load_id, |t| t.vocals = Some(vocals.clone())).is_some();
    if given {
        let _ = state.send_if_running(Command::SetVocals { deck, vocals: Some(vocals) });
    }
    given
}
