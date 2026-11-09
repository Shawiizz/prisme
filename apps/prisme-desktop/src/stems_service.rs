//! Stem separation in the background: a queue of tracks worked through on one thread, on
//! the GPU when there is a usable one, and the models it needs, each downloaded once. The
//! vocals model separates the vocals from the instrumental; the drums and bass model then
//! splits the instrumental into drums, bass and the other instruments.
//!
//! The separations are stored next to the tracks (`prisme_stems::track`): a track loaded
//! on a deck gets its stems as soon as they exist.

use crate::dto::{
    DeckTrackUpdateDto, ModelStatusDto, SeparatedDto, SeparationDto, StemsProgressDto, StemsStatusDto,
};
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState};
use crate::track_loading::DECK_TRACK_UPDATE_EVENT;
use prisme_engine::{Command, Stem, StemTracks};
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
/// Payload: [`SeparatedDto`].
pub const STEMS_SEPARATED_EVENT: &str = "stems-separated";
/// Payload: the [`ModelStatusDto`] of one model.
pub const STEMS_MODEL_EVENT: &str = "stems-model";

/// A separation model, converted by `crates/prisme-stems/tools/export_roformer.py --half`.
struct Model {
    id: &'static str,
    file: &'static str,
    url: &'static str,
    sha256: &'static str,
    bytes: u64,
    /// What it separates, in the order of its masks.
    stems: &'static [Stem],
}

const MODELS: [Model; 2] = [
    // the Mel-Band RoFormer vocals model of Kimberley Jensen (MIT licence)
    Model {
        id: "vocals",
        file: "roformer-vocals-f16-1.safetensors",
        url: "https://github.com/Shawiizz/prisme/releases/download/model-roformer-vocals-1/roformer-vocals-f16-1.safetensors",
        sha256: "90397548e9e427b2645ba33a71f5e3de3f1e01997f7c28fc4b0914fee7ba21ee",
        bytes: 456_488_724,
        stems: &[Stem::Vocals],
    },
    // the drums and bass of SYH99999's four-stem Mel-Band RoFormer (Apache 2.0 licence),
    // `--stems drums,bass --overlap 4`
    Model {
        id: "drums-bass",
        file: "roformer-drums-bass-f16-1.safetensors",
        url: "https://github.com/Shawiizz/prisme/releases/download/model-roformer-drums-bass-1/roformer-drums-bass-f16-1.safetensors",
        sha256: "b25942f2582deaac642b87697fff71a4a7e980358b85f27e5f8a00709944e3c1",
        bytes: 978_627_756,
        stems: &[Stem::Drums, Stem::Bass],
    },
];

pub struct StemsService {
    app: AppHandle,
    folder: PathBuf,
    queue: Arc<Mutex<Queue>>,
    cancel: Arc<AtomicBool>,
    downloads: Arc<Mutex<[Download; MODELS.len()]>>,
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
        Self {
            app,
            folder: data_dir.unwrap_or(Path::new(".")).join("models"),
            queue: Arc::default(),
            cancel: Arc::default(),
            downloads: Arc::default(),
        }
    }

    pub fn status(&self) -> StemsStatusDto {
        StemsStatusDto {
            models: (0..MODELS.len()).map(|m| self.model_status(m)).collect(),
            progress: lock(&self.queue).progress.clone(),
        }
    }

    fn model_path(&self, model: usize) -> PathBuf {
        self.folder.join(MODELS[model].file)
    }

    fn installed(&self) -> Vec<usize> {
        (0..MODELS.len()).filter(|&m| self.model_path(m).is_file()).collect()
    }

    fn model_status(&self, model: usize) -> ModelStatusDto {
        let download = &lock(&self.downloads)[model];
        ModelStatusDto {
            id: MODELS[model].id,
            installed: self.model_path(model).is_file(),
            downloading: download.running.then_some(download.fraction),
            error: download.error.clone(),
            bytes: MODELS[model].bytes,
        }
    }

    /// Queues the tracks that an installed model has stems to add to; starts working if idle.
    pub fn enqueue(&self, paths: impl IntoIterator<Item = PathBuf>) -> AppResult<usize> {
        let installed = self.installed();
        if installed.is_empty() {
            return Err(AppError::NoStemsModel);
        }
        let mut queue = lock(&self.queue);
        let before = queue.waiting.len();
        let current = queue.progress.current.clone();
        for path in paths {
            let already = queue.waiting.contains(&path)
                || current.as_deref() == Some(&*path.to_string_lossy())
                || missing(&installed, &path).is_empty();
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
                models: (0..MODELS.len()).map(|m| self.model_path(m)).collect(),
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

    /// Downloads the model `id` in the background (`stems-model` events); nothing if it is
    /// there or on its way.
    pub fn download_model(&self, id: &str) -> AppResult<()> {
        let model = MODELS
            .iter()
            .position(|m| m.id == id)
            .ok_or_else(|| AppError::Other(format!("no separation model {id}")))?;
        {
            let download = &mut lock(&self.downloads)[model];
            if download.running || self.model_path(model).is_file() {
                return Ok(());
            }
            *download = Download { running: true, ..Default::default() };
        }
        let (app, target, downloads) = (self.app.clone(), self.model_path(model), self.downloads.clone());
        tauri::async_runtime::spawn(async move {
            let result = fetch_model(&app, model, &target, &downloads).await;
            let mut state = lock(&downloads);
            state[model].running = false;
            if let Err(e) = result {
                error!("download of the {} model failed: {e}", MODELS[model].id);
                state[model].error = Some(e);
            }
            drop(state);
            let _ = app.emit(STEMS_MODEL_EVENT, app.state::<StemsService>().model_status(model));
        });
        Ok(())
    }
}

/// The models of `installed` with stems `track` lacks.
fn missing(installed: &[usize], track: &Path) -> Vec<usize> {
    installed
        .iter()
        .copied()
        .filter(|&m| MODELS[m].stems.iter().any(|s| !prisme_stems::track::has_stem(track, s.name())))
        .collect()
}

async fn fetch_model(
    app: &AppHandle,
    model: usize,
    target: &Path,
    downloads: &Mutex<[Download; MODELS.len()]>,
) -> Result<(), String> {
    let Model { url, sha256, bytes, .. } = MODELS[model];
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let folder = target.parent().ok_or("no folder for the model")?;
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let partial = target.with_extension("part");
    let client = reqwest::Client::builder().user_agent("Prisme").build().map_err(|e| e.to_string())?;
    let mut response =
        client.get(url).send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
    let mut file = std::fs::File::create(&partial).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut received = 0u64;
    let mut reported = 0.0;
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        hash.update(&chunk);
        received += chunk.len() as u64;
        let fraction = received as f64 / bytes as f64;
        if fraction - reported >= 0.005 {
            reported = fraction;
            lock(downloads)[model].fraction = fraction;
            let _ = app.emit(STEMS_MODEL_EVENT, app.state::<StemsService>().model_status(model));
        }
    }
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if received != bytes || digest != sha256 {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("the downloaded model is damaged ({received} bytes)"));
    }
    std::fs::rename(&partial, target).map_err(|e| e.to_string())?;
    info!("separation model {} installed in {}", MODELS[model].id, target.display());
    Ok(())
}

struct Worker {
    app: AppHandle,
    /// Where each of [`MODELS`] is (installed or not).
    models: Vec<PathBuf>,
    queue: Arc<Mutex<Queue>>,
    cancel: Arc<AtomicBool>,
}

type BoxedSeparator = Separator<Box<dyn MaskNetwork>>;

impl Worker {
    fn run(self) {
        prisme_library::background_priority();
        let mut separators: Vec<Option<BoxedSeparator>> = (0..MODELS.len()).map(|_| None).collect();
        while let Some(path) = self.next() {
            let installed: Vec<usize> = (0..MODELS.len()).filter(|&m| self.models[m].is_file()).collect();
            let needed = missing(&installed, &path);
            if needed.is_empty() {
                continue;
            }
            if let Err(e) = self.open(&mut separators, &needed) {
                error!("separation unavailable: {e}");
                let dropped = std::mem::take(&mut lock(&self.queue).waiting).len();
                self.update(|p| p.failed += 1 + dropped);
                break;
            }
            let mut chosen: Vec<&mut BoxedSeparator> = separators
                .iter_mut()
                .enumerate()
                .filter(|(m, _)| needed.contains(m))
                .filter_map(|(_, s)| s.as_mut())
                .collect();
            let result = prisme_stems::track::separate_file(&mut chosen, &path, |done, total| {
                self.update(|p| p.fraction = done as f64 / total as f64);
                !self.cancel.load(Ordering::Relaxed)
            });
            match result {
                Ok(_) => {
                    self.update(|p| p.done += 1);
                    let separated = SeparatedDto {
                        path: path.display().to_string(),
                        separation: SeparationDto::stored(&path),
                    };
                    let _ = self.app.emit(STEMS_SEPARATED_EVENT, separated);
                    attach_stems(&self.app, &path);
                }
                Err(StemsError::Cancelled) => info!("separation of {} cancelled", path.display()),
                Err(e) => {
                    warn!("{} not separated: {e}", path.display());
                    self.update(|p| p.failed += 1);
                }
            }
        }
        // the separators (and the GPU memory they hold) go with the thread
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

    /// Opens the separators of the `needed` models not open yet, on the GPU when it can.
    fn open(&self, separators: &mut [Option<BoxedSeparator>], needed: &[usize]) -> Result<(), StemsError> {
        for &m in needed {
            if separators[m].is_some() {
                continue;
            }
            let weights = Weights::load(&self.models[m])?;
            let stems: Vec<&str> = MODELS[m].stems.iter().map(|s| s.name()).collect();
            if weights.geometry.stems != stems {
                return Err(StemsError::Model(format!("{} does not separate {stems:?}", MODELS[m].file)));
            }
            let (network, device): (Box<dyn MaskNetwork>, String) = match GpuNetwork::new(weights.clone()) {
                Ok(gpu) => {
                    let device = gpu.adapter().to_owned();
                    (Box::new(gpu), device)
                }
                Err(e) => {
                    warn!("separating with the {} model on the CPU: {e}", MODELS[m].id);
                    (Box::new(CpuNetwork::new(weights)), "CPU".to_owned())
                }
            };
            info!("separating with the {} model on {device}", MODELS[m].id);
            self.update(|p| p.device = Some(device));
            separators[m] = Some(Separator::new(network));
        }
        Ok(())
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

/// Gives the stored stems of `path` to the decks it is loaded on (once fully decoded:
/// otherwise the end of its loading does it).
pub fn attach_stems(app: &AppHandle, path: &Path) {
    let state = app.state::<AppState>();
    let loaded: Vec<(usize, u64, Arc<prisme_media::TrackAudio>)> = lock(&state.tracks)
        .iter()
        .enumerate()
        .filter_map(|(deck, t)| {
            t.as_ref().filter(|t| t.path == path).map(|t| (deck, t.load_id, t.audio.clone()))
        })
        .collect();
    for (deck, load_id, audio) in loaded {
        if !audio.is_complete() {
            continue;
        }
        if let Some(separation) = give_stems(app, deck, load_id, path, &audio) {
            let update = DeckTrackUpdateDto {
                deck,
                load_id,
                waveform_ready: false,
                analysis: None,
                separation: Some(separation),
            };
            let _ = app.emit(DECK_TRACK_UPDATE_EVENT, update);
        }
    }
}

/// Loads the stored stems of the track loaded on `deck` as `load_id` and sends them to the
/// engine; how far it is separated if it got some (the caller tells the UI).
pub fn give_stems(
    app: &AppHandle,
    deck: usize,
    load_id: u64,
    path: &Path,
    audio: &prisme_media::TrackAudio,
) -> Option<SeparationDto> {
    let stems: StemTracks = Stem::ALL.map(|stem| {
        match prisme_stems::track::load_stem(path, stem.name(), audio.frames(), audio.sample_rate()) {
            Ok(stem) => stem.map(Arc::new),
            Err(e) => {
                warn!("{} of {} not read: {e}", stem.name(), path.display());
                None
            }
        }
    });
    let separation = SeparationDto::of(|stem| stems[stem as usize].is_some());
    if separation == SeparationDto::None {
        return None;
    }
    let state = app.state::<AppState>();
    state.with_loaded(deck, load_id, |t| t.stems = stems.clone())?;
    let _ = state.send_if_running(Command::SetStems { deck, stems });
    Some(separation)
}
