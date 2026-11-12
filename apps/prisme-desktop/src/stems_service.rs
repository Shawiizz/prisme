//! Stem separation in the background: a queue of tracks worked through on one thread, on
//! the GPU when there is a usable one, and the models it needs, each downloaded once. The
//! vocals model separates the vocals from the instrumental; the drums and bass model then
//! splits the instrumental into drums, bass and the other instruments.
//!
//! A track loaded on a deck goes before the library's: a library track being separated
//! gives way to it (and starts again afterwards). It is separated from where the deck plays,
//! both models abreast, and its stems reach the deck as they come
//! (`prisme_stems::track::TrackSeparation`).
//!
//! The models stay open a few minutes after the queue empties: the next track loaded starts
//! at once.
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
use prisme_media::TrackAudio;
use prisme_stems::track::TrackSeparation;
use prisme_stems::{CpuNetwork, Gpu, GpuNetwork, MaskNetwork, Separator, StemsError, Weights};
use sha2::{Digest, Sha256};
use std::collections::{HashSet, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tracing::{error, info, warn};

pub const STEMS_PROGRESS_EVENT: &str = "stems-progress";
/// Payload: [`SeparatedDto`].
pub const STEMS_SEPARATED_EVENT: &str = "stems-separated";
/// Payload: the [`ModelStatusDto`] of one model.
pub const STEMS_MODEL_EVENT: &str = "stems-model";
/// Longest track separated as soon as it is loaded: an hour-long mix would hold gigabytes
/// while separated (the library's queue still takes it).
const LONGEST_PLAYED_S: f64 = 20.0 * 60.0;
/// How long the worker keeps the models open, waiting for another track.
const IDLE: Duration = Duration::from_secs(300);

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
    /// Wakes the worker waiting for a track.
    wake: Arc<Condvar>,
    cancel: Arc<AtomicBool>,
    downloads: Arc<Mutex<[Download; MODELS.len()]>>,
}

#[derive(Default)]
struct Queue {
    /// Tracks loaded on a deck: separated first, in the order they were loaded.
    played: VecDeque<PathBuf>,
    /// The library's tracks.
    waiting: VecDeque<PathBuf>,
    progress: StemsProgressDto,
    /// The track being separated and its stems as the decks play them while they come.
    live: Option<(PathBuf, StemTracks)>,
    /// The worker running (separating, or waiting for a track with the models open), by
    /// number: one that stopped tells it apart from the one started after it.
    worker: Option<u64>,
    /// Workers started so far.
    started: u64,
}

impl Queue {
    fn len(&self) -> usize {
        self.played.len() + self.waiting.len()
    }

    /// The tracks queued or being separated.
    fn held(&self) -> HashSet<PathBuf> {
        let current = self.progress.current.as_deref().map(PathBuf::from);
        self.played.iter().chain(&self.waiting).cloned().chain(current).collect()
    }
}

#[derive(Default)]
struct Download {
    running: bool,
    fraction: f64,
    error: Option<String>,
}

impl StemsService {
    /// The service with its models in `folder`.
    pub fn new(app: AppHandle, folder: PathBuf) -> Self {
        Self {
            app,
            folder,
            queue: Arc::default(),
            wake: Arc::default(),
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
        // the files are looked at before taking the queue, which the worker needs too
        let wanted: Vec<PathBuf> = paths.into_iter().filter(|p| !missing(&installed, p).is_empty()).collect();
        let mut queue = lock(&self.queue);
        let mut held = queue.held();
        let before = queue.waiting.len();
        for path in wanted {
            if held.insert(path.clone()) {
                queue.waiting.push_back(path);
            }
        }
        let added = queue.waiting.len() - before;
        if added > 0 {
            self.work(&mut queue)?;
        }
        Ok(added)
    }

    /// Separates `path`, loaded on a deck and `duration_s` long, before the library's
    /// tracks, if an installed model has stems to add to it.
    pub fn separate_played(&self, path: &Path, duration_s: f64) -> AppResult<()> {
        let installed = self.installed();
        if duration_s > LONGEST_PLAYED_S || installed.is_empty() || missing(&installed, path).is_empty() {
            return Ok(());
        }
        let mut queue = lock(&self.queue);
        if queue.played.iter().any(|p| p == path)
            || queue.progress.current.as_deref() == Some(&*path.to_string_lossy())
        {
            return Ok(());
        }
        queue.played.push_back(path.to_owned());
        self.work(&mut queue)
    }

    /// Wakes the worker, or starts it, and tells the UI how the queue stands.
    fn work(&self, queue: &mut Queue) -> AppResult<()> {
        let running = queue.progress.running;
        queue.progress.queued = queue.len() + usize::from(running);
        if !running {
            let device = queue.progress.device.take();
            queue.progress =
                StemsProgressDto { running: true, queued: queue.len(), device, ..Default::default() };
            self.cancel.store(false, Ordering::Relaxed);
        }
        if queue.worker.is_none() {
            let id = queue.started;
            queue.started += 1;
            let worker = Worker {
                id,
                app: self.app.clone(),
                models: (0..MODELS.len()).map(|m| self.model_path(m)).collect(),
                queue: self.queue.clone(),
                wake: self.wake.clone(),
                cancel: self.cancel.clone(),
            };
            std::thread::Builder::new()
                .name("prisme-stems".into())
                .spawn(move || worker.run())
                .map_err(|e| AppError::Other(e.to_string()))?;
            queue.worker = Some(id);
        }
        self.wake.notify_one();
        let _ = self.app.emit(STEMS_PROGRESS_EVENT, queue.progress.clone());
        Ok(())
    }

    /// Empties the queue and stops the track being separated.
    pub fn cancel(&self) {
        let mut queue = lock(&self.queue);
        queue.waiting.clear();
        queue.played.clear();
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// The stems of `path` as they come, if it is being separated.
    fn live(&self, path: &Path) -> Option<StemTracks> {
        lock(&self.queue).live.as_ref().filter(|(p, _)| p == path).map(|(_, stems)| stems.clone())
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

/// A stalled connection gives up after this long, and the download resumes where it stopped.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);
/// Connections a download may take, each resuming the last.
const DOWNLOAD_ATTEMPTS: usize = 3;

async fn fetch_model(
    app: &AppHandle,
    model: usize,
    target: &Path,
    downloads: &Mutex<[Download; MODELS.len()]>,
) -> Result<(), String> {
    let Model { sha256, bytes, .. } = MODELS[model];
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let folder = target.parent().ok_or("no folder for the model")?;
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let partial = target.with_extension("part");
    let client = reqwest::Client::builder()
        .user_agent("Prisme")
        .connect_timeout(DOWNLOAD_TIMEOUT)
        .read_timeout(DOWNLOAD_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    for attempt in 1.. {
        match fetch_rest(app, &client, model, &partial, downloads).await {
            Ok(()) => break,
            Err(e) if attempt < DOWNLOAD_ATTEMPTS => {
                warn!("download of the {} model cut: {e}", MODELS[model].id)
            }
            Err(e) => return Err(e),
        }
    }
    let check = partial.clone();
    let (received, digest) = tauri::async_runtime::spawn_blocking(move || sha256_of(&check))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    if received != bytes || digest != sha256 {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("the downloaded model is damaged ({received} bytes)"));
    }
    std::fs::rename(&partial, target).map_err(|e| e.to_string())?;
    info!("separation model {} installed in {}", MODELS[model].id, target.display());
    Ok(())
}

/// Appends to `partial` the rest of the model: all of it, or from where an earlier download
/// stopped if the server sends parts.
async fn fetch_rest(
    app: &AppHandle,
    client: &reqwest::Client,
    model: usize,
    partial: &Path,
    downloads: &Mutex<[Download; MODELS.len()]>,
) -> Result<(), String> {
    let Model { url, bytes, .. } = MODELS[model];
    let start = std::fs::metadata(partial).map_or(0, |m| m.len());
    if start >= bytes {
        return Ok(());
    }
    let mut request = client.get(url);
    if start > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={start}-"));
    }
    let mut response = request.send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
    let resumed = response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resumed)
        .truncate(!resumed)
        .open(partial)
        .map_err(|e| e.to_string())?;
    let mut received = if resumed { start } else { 0 };
    let mut reported = 0.0;
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        received += chunk.len() as u64;
        let fraction = received as f64 / bytes as f64;
        if fraction - reported >= 0.005 {
            reported = fraction;
            lock(downloads)[model].fraction = fraction;
            let _ = app.emit(STEMS_MODEL_EVENT, app.state::<StemsService>().model_status(model));
        }
    }
    file.sync_all().map_err(|e| e.to_string())
}

/// The length and SHA-256 (in hex) of a file.
fn sha256_of(path: &Path) -> std::io::Result<(u64, String)> {
    let mut hash = Sha256::new();
    let received = std::io::copy(&mut std::fs::File::open(path)?, &mut hash)?;
    Ok((received, hash.finalize().iter().map(|b| format!("{b:02x}")).collect()))
}

struct Worker {
    id: u64,
    app: AppHandle,
    /// Where each of [`MODELS`] is (installed or not).
    models: Vec<PathBuf>,
    queue: Arc<Mutex<Queue>>,
    wake: Arc<Condvar>,
    cancel: Arc<AtomicBool>,
}

type BoxedSeparator = Separator<Box<dyn MaskNetwork>>;

/// A track taken from the queue.
struct Job {
    path: PathBuf,
    /// Loaded on a deck when queued.
    played: bool,
    /// In the library's queue too.
    requested: bool,
}

/// How a separation ended, short of an error.
enum Ended {
    /// Every stem stored.
    Through,
    /// Given way to a track loaded on a deck: back in the queue.
    GaveWay,
    /// Off the decks, and nobody asked for it otherwise.
    Dropped,
}

impl Worker {
    fn run(self) {
        prisme_library::background_priority();
        let _stopped = Stopped { id: self.id, app: self.app.clone(), queue: self.queue.clone() };
        let mut separators: Vec<Option<BoxedSeparator>> = (0..MODELS.len()).map(|_| None).collect();
        let open_gpu =
            || Gpu::open(Default::default()).inspect_err(|e| warn!("separating on the CPU: {e}")).ok();
        let mut gpu = open_gpu();
        let mut gpu_failed = false;
        while let Some(job) = self.next() {
            let installed: Vec<usize> = (0..MODELS.len()).filter(|&m| self.models[m].is_file()).collect();
            let needed = missing(&installed, &job.path);
            if needed.is_empty() {
                continue;
            }
            if let Err(e) = self.open(gpu.as_ref(), &mut separators, &needed) {
                error!("separation unavailable: {e}");
                let dropped = {
                    let mut queue = lock(&self.queue);
                    let dropped = queue.len();
                    queue.played.clear();
                    queue.waiting.clear();
                    dropped
                };
                self.update(|p| p.failed += 1 + dropped);
                break;
            }
            let mut chosen: Vec<&mut BoxedSeparator> = separators
                .iter_mut()
                .enumerate()
                .filter(|(m, _)| needed.contains(m))
                .filter_map(|(_, s)| s.as_mut())
                .collect();
            let result = self.separate(&mut chosen, &job);
            lock(&self.queue).live = None;
            match result {
                Ok(Ended::Through) => {
                    self.update(|p| p.done += 1);
                    let separation = SeparationDto::stored(&job.path);
                    let separated = SeparatedDto { path: job.path.display().to_string(), separation };
                    let _ = self.app.emit(STEMS_SEPARATED_EVENT, separated);
                    attach_stems(&self.app, &job.path);
                }
                Ok(Ended::GaveWay) => {
                    info!("separation of {} gives way to a track on a deck", job.path.display());
                    lock(&self.queue).waiting.push_front(job.path);
                }
                Ok(Ended::Dropped) => info!("{} left the decks before its separation", job.path.display()),
                Err(StemsError::Cancelled) => info!("separation of {} cancelled", job.path.display()),
                Err(StemsError::Gpu(e)) => {
                    // out of memory, a driver reset: the GPU starts afresh, the CPU after
                    // a second failure; the track is tried again first
                    warn!("separation of {} failed on the GPU: {e}", job.path.display());
                    separators.iter_mut().for_each(|s| *s = None);
                    gpu = if gpu_failed { None } else { open_gpu() };
                    gpu_failed = true;
                    let mut queue = lock(&self.queue);
                    let line = if job.played { &mut queue.played } else { &mut queue.waiting };
                    line.push_front(job.path);
                }
                Err(e) => {
                    warn!("{} not separated: {e}", job.path.display());
                    self.update(|p| p.failed += 1);
                }
            }
        }
    }

    /// The next track, the decks' first: waits for one up to [`IDLE`].
    fn next(&self) -> Option<Job> {
        let mut queue = lock(&self.queue);
        let job = loop {
            if let Some(path) = queue.played.pop_front() {
                let before = queue.waiting.len();
                queue.waiting.retain(|p| *p != path);
                let requested = queue.waiting.len() < before;
                break Job { path, played: true, requested };
            }
            if let Some(path) = queue.waiting.pop_front() {
                break Job { path, played: false, requested: true };
            }
            if queue.progress.running {
                idle(&mut queue.progress);
                let _ = self.app.emit(STEMS_PROGRESS_EVENT, queue.progress.clone());
            }
            let (woken, wait) = self.wake.wait_timeout(queue, IDLE).unwrap_or_else(|e| e.into_inner());
            queue = woken;
            if wait.timed_out() && queue.len() == 0 {
                // under the lock, so that a track queued from now starts another worker
                queue.worker = None;
                return None;
            }
        };
        queue.progress.current = Some(job.path.display().to_string());
        queue.progress.fraction = 0.0;
        queue.progress.queued = queue.len() + 1;
        drop(queue);
        self.update(|_| {});
        Some(job)
    }

    /// Separates the job's track, its stems reaching the decks it is loaded on as they come.
    fn separate(&self, separators: &mut [&mut BoxedSeparator], job: &Job) -> Result<Ended, StemsError> {
        let deck = self.on_decks(&job.path);
        let mut separation = TrackSeparation::open(
            separators,
            &job.path,
            deck.as_ref().map(|(audio, _)| (audio.sample_rate(), samples(audio))),
            deck.map(|(_, frame)| frame),
        )?;
        let mut live = None;
        while !separation.is_done() {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(StemsError::Cancelled);
            }
            let on_decks = self.on_decks(&job.path).is_some();
            if !on_decks {
                if job.played && !job.requested {
                    return Ok(Ended::Dropped);
                }
                if !lock(&self.queue).played.is_empty() {
                    return Ok(Ended::GaveWay);
                }
            }
            if on_decks && live.is_none() {
                live = Some(Live::start(&self.app, &separation, &self.queue)?);
            }
            separation.step(separators)?;
            if let Some(live) = &live {
                live.publish(&separation)?;
            }
            separation.store_through()?;
            self.update(|p| p.fraction = separation.done() as f64 / separation.chunks() as f64);
        }
        Ok(Ended::Through)
    }

    /// The audio of `path` on a deck, fully decoded, and the frame the deck plays.
    fn on_decks(&self, path: &Path) -> Option<(Arc<TrackAudio>, usize)> {
        let state = self.app.state::<AppState>();
        let (deck, audio) = lock(&state.tracks)
            .iter()
            .enumerate()
            .find_map(|(deck, t)| t.as_ref().filter(|t| t.path == path).map(|t| (deck, t.audio.clone())))?;
        if !audio.is_complete() {
            return None;
        }
        let position = state.deck_status(deck).map_or(0.0, |s| s.position_s.max(0.0));
        let frame =
            ((position * f64::from(audio.sample_rate())) as usize).min(audio.frames().saturating_sub(1));
        Some((audio, frame))
    }

    /// Opens the separators of the `needed` models not open yet, all at once, on `gpu` when
    /// they can: they share it, their chunks taking turns.
    fn open(
        &self,
        gpu: Option<&Arc<Gpu>>,
        separators: &mut [Option<BoxedSeparator>],
        needed: &[usize],
    ) -> Result<(), StemsError> {
        let missing: Vec<usize> = needed.iter().copied().filter(|&m| separators[m].is_none()).collect();
        let opened = std::thread::scope(|scope| {
            let opening: Vec<_> =
                missing.iter().map(|&m| (m, scope.spawn(move || self.open_model(gpu, m)))).collect();
            opening
                .into_iter()
                .map(|(m, opening)| (m, opening.join().expect("a model opens without panicking")))
                .collect::<Vec<_>>()
        });
        for (m, opened) in opened {
            let (network, device) = opened?;
            info!("separating with the {} model on {device}", MODELS[m].id);
            self.update(|p| p.device = Some(device));
            separators[m] = Some(Separator::new(network));
        }
        Ok(())
    }

    /// Model `m` as a network, on `gpu` if it can, and where it runs.
    fn open_model(
        &self,
        gpu: Option<&Arc<Gpu>>,
        m: usize,
    ) -> Result<(Box<dyn MaskNetwork>, String), StemsError> {
        let weights = Weights::load(&self.models[m])?;
        let stems: Vec<&str> = MODELS[m].stems.iter().map(|s| s.name()).collect();
        if weights.geometry.stems != stems {
            return Err(StemsError::Model(format!("{} does not separate {stems:?}", MODELS[m].file)));
        }
        let on_gpu = gpu.map(|gpu| GpuNetwork::on(gpu, &weights)).transpose().unwrap_or_else(|e| {
            warn!("separating with the {} model on the CPU: {e}", MODELS[m].id);
            None
        });
        Ok(match on_gpu {
            Some(network) => {
                let device = network.adapter().to_owned();
                (Box::new(network), device)
            }
            None => (Box::new(CpuNetwork::with_pool(weights, background_pool()?)), "CPU".to_owned()),
        })
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

/// The stems of a track being separated, given to the decks it is loaded on: those still
/// separated fill in as they come, the others are read from their files.
struct Live {
    /// The stems being separated, by their index in `TrackSeparation::stems`.
    filling: Vec<Option<Arc<TrackAudio>>>,
}

impl Live {
    fn start(
        app: &AppHandle,
        separation: &TrackSeparation,
        queue: &Mutex<Queue>,
    ) -> Result<Self, StemsError> {
        let path = separation.track();
        let (rate, frames) = (separation.sample_rate(), separation.frames());
        let separated = separation.stems();
        let filling: Vec<Option<Arc<TrackAudio>>> = (0..separated.len())
            .map(|i| (!separation.is_stored(i)).then(|| Arc::new(TrackAudio::pending(rate, frames))))
            .collect();
        let stems: StemTracks = Stem::ALL.map(|stem| {
            let filled =
                separated.iter().position(|(name, _)| *name == stem.name()).and_then(|i| filling[i].clone());
            filled.or_else(|| {
                prisme_stems::track::load_stem(path, stem.name(), frames, rate)
                    .unwrap_or_else(|e| {
                        warn!("{} of {} not read: {e}", stem.name(), path.display());
                        None
                    })
                    .map(Arc::new)
            })
        });
        lock(queue).live = Some((path.to_owned(), stems.clone()));
        let live = Self { filling };
        live.publish(separation)?;
        let state = app.state::<AppState>();
        let decks: Vec<(usize, u64)> = lock(&state.tracks)
            .iter()
            .enumerate()
            .filter_map(|(deck, t)| t.as_ref().filter(|t| t.path == path).map(|t| (deck, t.load_id)))
            .collect();
        for (deck, load_id) in decks {
            if let Some(separation) = give(app, deck, load_id, &stems, frames, rate) {
                tell(app, deck, load_id, separation);
            }
        }
        Ok(live)
    }

    /// Fills in what is final of the stems since last time.
    fn publish(&self, separation: &TrackSeparation) -> Result<(), StemsError> {
        for (index, ((_, finished), audio)) in separation.stems().into_iter().zip(&self.filling).enumerate() {
            let Some(audio) = audio else { continue };
            let readable = audio.readable();
            let parts = if readable.is_empty() {
                [finished.clone(), 0..0]
            } else {
                [finished.start..readable.start, readable.end..finished.end]
            };
            for part in parts.into_iter().filter(|p| !p.is_empty()) {
                audio.publish(part.start, &separation.read(index, part)?);
            }
        }
        Ok(())
    }
}

/// Tells the queue the worker stopped, whether it returned or panicked: the next track starts
/// another one. The separators (and the GPU memory they hold) go with the thread. A worker
/// that gave the queue up while waiting has nothing to tell: another may run already.
struct Stopped {
    id: u64,
    app: AppHandle,
    queue: Arc<Mutex<Queue>>,
}

impl Drop for Stopped {
    fn drop(&mut self) {
        let mut queue = lock(&self.queue);
        if queue.worker != Some(self.id) {
            return;
        }
        queue.worker = None;
        queue.live = None;
        idle(&mut queue.progress);
        let _ = self.app.emit(STEMS_PROGRESS_EVENT, queue.progress.clone());
    }
}

/// Threads for a separation on the CPU: all the cores but two, at a background priority.
fn background_pool() -> Result<rayon::ThreadPool, StemsError> {
    let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
    rayon::ThreadPoolBuilder::new()
        .num_threads(cores.saturating_sub(2).max(1))
        .thread_name(|i| format!("prisme-stems-cpu-{i}"))
        .start_handler(|_| prisme_library::background_priority())
        .build()
        .map_err(|e| StemsError::Track(e.to_string()))
}

/// Nothing left to separate.
fn idle(progress: &mut StemsProgressDto) {
    progress.running = false;
    progress.current = None;
    progress.queued = 0;
}

/// The interleaved samples of a decoded track.
fn samples(audio: &TrackAudio) -> Vec<f32> {
    let mut samples = Vec::with_capacity(audio.frames() * 2);
    audio.for_each_block(1 << 16, |block| samples.extend_from_slice(block));
    samples
}

/// Gives the stored stems of `path` to the decks it is loaded on (once fully decoded:
/// otherwise the end of its loading does it), but for those that got them as they came.
pub fn attach_stems(app: &AppHandle, path: &Path) {
    let stored = SeparationDto::stored(path);
    let state = app.state::<AppState>();
    let mut audio = None;
    let loads: Vec<(usize, u64)> = lock(&state.tracks)
        .iter()
        .enumerate()
        .filter_map(|(deck, t)| {
            let t = t
                .as_ref()
                .filter(|t| t.path == path && t.audio.is_complete() && !plays_whole(&t.stems, stored))?;
            audio.get_or_insert_with(|| t.audio.clone());
            Some((deck, t.load_id))
        })
        .collect();
    let Some(audio) = audio else { return };
    for (deck, load_id, separation) in give_stems(app, &loads, path, &audio) {
        tell(app, deck, load_id, separation);
    }
}

/// Whether `stems` are whole and go as far as `separation`.
fn plays_whole(stems: &StemTracks, separation: SeparationDto) -> bool {
    SeparationDto::of(|s| stems[s as usize].is_some()) == separation
        && stems.iter().flatten().all(|s| s.is_complete())
}

/// Gives the decks playing `loads` of `path` (its decoded `audio`) its stems: as they come if
/// it is being separated, else those stored, read once for them all; how far each deck it
/// separates is (the caller tells the UI).
pub fn give_stems(
    app: &AppHandle,
    loads: &[(usize, u64)],
    path: &Path,
    audio: &TrackAudio,
) -> Vec<(usize, u64, SeparationDto)> {
    if loads.is_empty() {
        return Vec::new();
    }
    let (frames, rate) = (audio.frames(), audio.sample_rate());
    let stems = app.state::<StemsService>().live(path).unwrap_or_else(|| {
        Stem::ALL.map(|stem| match prisme_stems::track::load_stem(path, stem.name(), frames, rate) {
            Ok(stem) => stem.map(Arc::new),
            Err(e) => {
                warn!("{} of {} not read: {e}", stem.name(), path.display());
                None
            }
        })
    });
    loads
        .iter()
        .filter_map(|&(deck, load_id)| Some((deck, load_id, give(app, deck, load_id, &stems, frames, rate)?)))
        .collect()
}

/// Gives `stems` to the deck playing `load_id` if they fit its track; how far they separate
/// it, if at all.
fn give(
    app: &AppHandle,
    deck: usize,
    load_id: u64,
    stems: &StemTracks,
    frames: usize,
    rate: u32,
) -> Option<SeparationDto> {
    let separation = SeparationDto::of(|stem| stems[stem as usize].is_some());
    if separation == SeparationDto::None {
        return None;
    }
    let state = app.state::<AppState>();
    state.with_loaded(deck, load_id, |t| {
        (t.audio.frames() == frames && t.audio.sample_rate() == rate).then(|| t.stems = stems.clone())
    })??;
    let _ = state.send_if_running(Command::SetStems { deck, stems: stems.clone() });
    Some(separation)
}

/// Tells the UI how far the track on `deck` is separated.
fn tell(app: &AppHandle, deck: usize, load_id: u64, separation: SeparationDto) {
    let update = DeckTrackUpdateDto {
        deck,
        load_id,
        waveform_ready: false,
        analysis: None,
        separation: Some(separation),
    };
    let _ = app.emit(DECK_TRACK_UPDATE_EVENT, update);
}
