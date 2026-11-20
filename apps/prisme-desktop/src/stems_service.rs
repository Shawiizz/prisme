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
//!
//! Tracks asked for while no model is installed are offered the models; once the DJ downloads
//! them, they wait for the downloads and go into the queue as they end. What the interface shows
//! of all this, [`Shown`], is mirrored in `AppState::stems` as it changes.

use crate::dto::{
    ChoiceDto, DeckStemsDto, ModelRowDto, ModelStatusDto, SeparateDto, SeparationDto, StemMixDto,
    StemsProgressDto, StemsStateDto,
};
use crate::error::{AppError, AppResult};
use crate::preferences::Preferences;
use crate::state::{lock, AppState, Section};
use prisme_engine::{Command, Stem, StemTracks};
use prisme_media::TrackAudio;
use prisme_stems::track::TrackSeparation;
use prisme_stems::{CpuNetwork, Gpu, GpuNetwork, GpuOptions, MaskNetwork, Separator, StemsError, Weights};
use sha2::{Digest, Sha256};
use std::collections::{HashSet, VecDeque};
use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tracing::{error, info, warn};

/// Longest track separated as soon as it is loaded: an hour-long mix would hold gigabytes
/// while separated (the library's queue still takes it).
const LONGEST_PLAYED_S: f64 = 20.0 * 60.0;
/// How long the worker keeps the models open, waiting for another track.
const IDLE: Duration = Duration::from_secs(300);
/// Chunks over each sample off the matrix units of a graphics card (another card, the
/// processor), where a model asking for four takes minutes: half the time for a difference
/// a blind test hardly heard.
const SLOW_OVERLAP: f64 = 2.0;

/// A separation model, converted by `crates/prisme-stems/tools/export_roformer.py --half` (or
/// `export_bs_roformer.py`).
struct Model {
    id: &'static str,
    file: &'static str,
    url: &'static str,
    sha256: &'static str,
    bytes: u64,
    /// What it separates, in the order of its masks.
    stems: &'static [Stem],
    /// A small model, faster, for stems large ones separate too.
    fast: bool,
}

const MODELS: [Model; 3] = [
    // the Mel-Band RoFormer vocals model of Kimberley Jensen (MIT licence)
    Model {
        id: "vocals",
        file: "roformer-vocals-f16-1.safetensors",
        url: "https://github.com/Shawiizz/prisme/releases/download/model-roformer-vocals-1/roformer-vocals-f16-1.safetensors",
        sha256: "90397548e9e427b2645ba33a71f5e3de3f1e01997f7c28fc4b0914fee7ba21ee",
        bytes: 456_488_724,
        stems: &[Stem::Vocals],
        fast: false,
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
        fast: false,
    },
    // HiDolen's Mini-BS-RoFormer-V2.5-46.2M (CC BY-NC 4.0 licence): vocals, drums and bass in
    // one pass of a model a sixteenth of the two others, for a phone or a computer without a
    // graphics card worth it; `--stems vocals,drums,bass`
    Model {
        id: "four-fast",
        file: "bs-roformer-four-f16-1.safetensors",
        url: "https://github.com/Shawiizz/prisme/releases/download/model-bs-roformer-four-1/bs-roformer-four-f16-1.safetensors",
        sha256: "fbdc57c3f02def5358feae39e5cb8e4ff01c4be86b2f89d97ae51059a04935e7",
        bytes: 91_323_924,
        stems: &[Stem::Vocals, Stem::Drums, Stem::Bass],
        fast: true,
    },
];

/// Of the installed models, those the separation uses: the ones the DJ chose (`fast`: the
/// small one), never the others in their place, which take another time and memory than the
/// DJ chose (the large ones sixteen times the small one's memory).
fn in_use(installed: &[usize], fast: bool) -> Vec<usize> {
    installed.iter().copied().filter(|&m| MODELS[m].fast == fast).collect()
}

pub struct StemsService {
    app: AppHandle,
    folder: PathBuf,
    queue: Arc<Mutex<Queue>>,
    /// Wakes the worker waiting for a track.
    wake: Arc<Condvar>,
    downloads: Arc<Mutex<[Download; MODELS.len()]>>,
    /// Stops the chunk the processor works on, for the worker to look again at what it does:
    /// a chunk takes it seconds, a large model's half a minute and more.
    interrupt: Arc<AtomicBool>,
}

#[derive(Default)]
struct Queue {
    /// Tracks loaded on a deck: separated first, in the order they were loaded.
    played: VecDeque<PathBuf>,
    /// The library's tracks.
    waiting: VecDeque<PathBuf>,
    /// The tracks asked for last while no model was installed: the models were offered for them.
    offered: Vec<PathBuf>,
    /// Tracks to separate once the models on their way are in.
    awaiting: Vec<PathBuf>,
    progress: StemsProgressDto,
    /// The track being separated and its stems as the decks play them while they come.
    live: Option<(PathBuf, StemTracks)>,
    /// The worker running (separating, or waiting for a track with the models open), by
    /// number: one that stopped tells it apart from the one started after it.
    worker: Option<u64>,
    /// Workers started so far.
    started: u64,
    /// Cancels asked so far: a job taken before the last one stops.
    cancels: u64,
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

    /// The next track, the decks' first, and of those a playing one's.
    fn take(&mut self, playing: &[PathBuf]) -> Option<Job> {
        let cancels = self.cancels;
        let first = self.played.iter().position(|p| playing.contains(p)).unwrap_or(0);
        if let Some(path) = self.played.remove(first) {
            let before = self.waiting.len();
            self.waiting.retain(|p| *p != path);
            let requested = self.waiting.len() < before;
            return Some(Job { path, played: true, requested, cancels });
        }
        let path = self.waiting.pop_front()?;
        Some(Job { path, played: false, requested: true, cancels })
    }

    /// Empties the queue and stops the job being done.
    fn cancel(&mut self) {
        self.waiting.clear();
        self.played.clear();
        self.cancels += 1;
    }

    fn cancelled(&self, job: &Job) -> bool {
        job.cancels != self.cancels
    }

    /// Puts the job's track back at the head of the decks' line or the library's, unless
    /// the queue was cancelled since.
    fn put_back(&mut self, job: Job, played: bool) {
        if !self.cancelled(&job) {
            let line = if played { &mut self.played } else { &mut self.waiting };
            line.push_front(job.path);
        }
    }
}

#[derive(Default)]
struct Download {
    running: bool,
    fraction: f64,
    error: Option<String>,
}

/// The graphics card could not be opened, or failed twice: the separation runs on the CPU.
static GPU_GAVE_UP: AtomicBool = AtomicBool::new(false);

/// The graphics chip separates on a computer and on Android (Vulkan); an iPhone's is left
/// alone until it has been tried.
const USES_GPU: bool = cfg!(any(desktop, target_os = "android"));

/// Whether the DJ lets the separation use the graphics chip.
fn gpu_allowed(state: &AppState) -> bool {
    USES_GPU && state.preferences.read(|p| p.library.stems_gpu)
}

static HAS_GPU: OnceLock<bool> = OnceLock::new();

/// Whether there is a graphics card worth separating on (looked for once, which takes a
/// moment: not from the window's thread), whether or not the DJ lets it.
pub fn gpu_found() -> bool {
    USES_GPU && *HAS_GPU.get_or_init(Gpu::available)
}

/// [`gpu_found`] if it was looked for already.
fn gpu_known() -> Option<bool> {
    if USES_GPU {
        HAS_GPU.get().copied()
    } else {
        Some(false)
    }
}

/// Looks for a graphics card off the window's thread as Prisme starts: the settings show
/// whether it separates there.
pub fn look_for_gpu(app: AppHandle) {
    let looking = std::thread::Builder::new().name("prisme-gpu".into()).spawn(move || {
        gpu_found();
        app.state::<StemsService>().show();
    });
    if let Err(e) = looking {
        warn!("the graphics card is looked for when first needed: {e}");
    }
}

/// Whether the separation runs on a graphics card: one is there, the DJ lets it, and it did
/// not give up since.
pub fn has_gpu(state: &AppState) -> bool {
    gpu_allowed(state) && gpu_found() && !GPU_GAVE_UP.load(Ordering::Relaxed)
}

/// Whether the DJ prefers the small models, faster, to the large ones.
fn prefers_fast(state: &AppState) -> bool {
    state.preferences.read(|p| p.library.stems_fast)
}

/// Whether tracks are separated as they are loaded or prepared: as the DJ chose, else with
/// a graphics card only (the processor alone takes minutes a track, on all its cores).
pub fn separates_on_load(state: &AppState) -> bool {
    state.preferences.read(|p| p.library.separate_on_load).unwrap_or_else(|| has_gpu(state))
}

/// The stems stored next to `track`: how far it is separated (a file looked for per stem).
pub fn stored_separation(track: &Path) -> SeparationDto {
    SeparationDto::of(|stem| prisme_stems::track::has_stem(track, stem.name()))
}

/// What the interface shows of the separation (see [`StemsService::show`]): the models, the
/// queue, and the tracks that wait for the models.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shown {
    /// In the order of [`MODELS`].
    models: Vec<ModelStatusDto>,
    progress: StemsProgressDto,
    /// The tracks queued or being separated, which the rows of the library mark.
    pub queued: Queued,
    awaiting: Vec<PathBuf>,
    /// Whether a graphics card was found, once looked for, and whether the separation gave it
    /// up since.
    gpu: Option<bool>,
    gave_up: bool,
}

/// A set of tracks shared as it was made: told apart from another by where it is, not by what
/// it holds (a library queued whole holds thousands).
#[derive(Debug, Clone, Default)]
pub struct Queued(pub Arc<HashSet<PathBuf>>);

impl PartialEq for Queued {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Queued {
    /// The tracks queued `now`: these very ones if they are the same, so that what shows them
    /// sees no change.
    fn again(&self, now: HashSet<PathBuf>) -> Self {
        if *self.0 == now {
            self.clone()
        } else {
            Self(Arc::new(now))
        }
    }
}

impl Shown {
    /// What the separation shows with the DJ's `preferences`.
    pub fn dto(&self, preferences: &Preferences) -> StemsStateDto {
        let library = &preferences.library;
        let installed = |m: usize| self.models.get(m).is_some_and(|s| s.installed);
        let gpu = self.gpu;
        let fast_suits = cfg!(mobile) || gpu == Some(false);
        let on_gpu = gpu == Some(true) && library.stems_gpu && !self.gave_up;
        let choice = |fast: bool| {
            let of = || (0..MODELS.len()).filter(move |&m| MODELS[m].fast == fast);
            let left: u64 = of().filter(|&m| !installed(m)).map(|m| MODELS[m].bytes).sum();
            ChoiceDto {
                bytes_left: (left > 0).then_some(left),
                error: of().find_map(|m| self.models.get(m)?.error.clone()),
            }
        };
        let vocals = MODELS.iter().position(|m| !m.fast && m.stems.contains(&Stem::Vocals));
        let models = (0..MODELS.len()).filter(|&m| MODELS[m].fast == library.stems_fast);
        let p = &self.progress;
        StemsStateDto {
            total: p.done + p.failed + p.queued,
            overall: self.overall(),
            progress: self.progress.clone(),
            reach: self.reach(library.stems_fast),
            gpu,
            on_gpu,
            on_load: library.separate_on_load.unwrap_or(on_gpu),
            downloading: self.downloading(),
            models: models
                .filter_map(|m| {
                    let ready = MODELS[m].stems.contains(&Stem::Vocals) || vocals.is_none_or(installed);
                    Some(ModelRowDto { status: self.models.get(m)?.clone(), ready })
                })
                .collect(),
            quality: choice(false),
            fast: choice(true),
            fast_suits,
            suggest_fast: library.stems_fast || fast_suits,
        }
    }

    /// How far the models installed separate a track, the small ones (`fast`) or the large.
    pub fn reach(&self, fast: bool) -> SeparationDto {
        let installed: Vec<usize> =
            (0..MODELS.len()).filter(|&m| self.models.get(m).is_some_and(|s| s.installed)).collect();
        reach(&in_use(&installed, fast))
    }

    /// The queue's share done, the track being separated as far as it is, from 0 to 1.
    fn overall(&self) -> f64 {
        let p = &self.progress;
        let total = p.done + p.failed + p.queued;
        let current = if p.current.is_some() { p.fraction } else { 0.0 };
        if total > 0 {
            ((p.done + p.failed) as f64 + current) / total as f64
        } else {
            0.0
        }
    }

    /// How far the downloads under way are, by their bytes, from 0 to 1; `None` without any.
    fn downloading(&self) -> Option<f64> {
        let under_way = || self.models.iter().filter_map(|s| Some((s.bytes as f64, s.downloading?)));
        let bytes: f64 = under_way().map(|(bytes, _)| bytes).sum();
        (bytes > 0.0).then(|| under_way().map(|(bytes, done)| bytes * done).sum::<f64>() / bytes)
    }

    /// How far the separation of `path` is, from 0 to 1: as it is separated, or as the downloads
    /// of the models it waits for are.
    pub fn separating(&self, path: &Path) -> Option<f64> {
        if self.progress.current.as_deref().is_some_and(|current| Path::new(current) == path) {
            Some(self.progress.fraction)
        } else if self.awaiting.iter().any(|p| p == path) {
            self.downloading()
        } else {
            None
        }
    }

    /// The track being separated, and how far, to a hundredth.
    pub fn current(&self) -> Option<(PathBuf, u8)> {
        let p = &self.progress;
        p.current.as_ref().map(|current| (PathBuf::from(current), (p.fraction * 100.0).floor() as u8))
    }
}

/// How far `models` separate a track: the vocals, then the drums and the bass with them.
fn reach(models: &[usize]) -> SeparationDto {
    let has = |stem: Stem| models.iter().any(|&m| MODELS[m].stems.contains(&stem));
    if !has(Stem::Vocals) {
        SeparationDto::None
    } else if has(Stem::Drums) && has(Stem::Bass) {
        SeparationDto::Full
    } else {
        SeparationDto::Vocals
    }
}

impl StemsService {
    /// The service with its models in `folder`.
    pub fn new(app: AppHandle, folder: PathBuf) -> Self {
        Self {
            app,
            folder,
            queue: Arc::default(),
            wake: Arc::default(),
            downloads: Arc::default(),
            interrupt: Arc::default(),
        }
    }

    /// Mirrors what the interface shows of the separation in `AppState::stems`. Never with the
    /// queue's lock held, which it takes.
    pub fn show(&self) {
        let (progress, queued, awaiting) = {
            let queue = lock(&self.queue);
            (queue.progress.clone(), queue.held(), queue.awaiting.clone())
        };
        let models = (0..MODELS.len()).map(|m| self.model_status(m)).collect();
        let state = self.app.state::<AppState>();
        let mut shown = lock(&state.stems);
        let queued = shown.queued.again(queued);
        let (gpu, gave_up) = (gpu_known(), GPU_GAVE_UP.load(Ordering::Relaxed));
        *shown = Shown { models, progress, queued, awaiting, gpu, gave_up };
    }

    fn model_path(&self, model: usize) -> PathBuf {
        self.folder.join(MODELS[model].file)
    }

    /// The installed models the separation uses (see [`in_use`]).
    fn installed(&self) -> Vec<usize> {
        let installed: Vec<usize> = (0..MODELS.len()).filter(|&m| self.model_path(m).is_file()).collect();
        in_use(&installed, prefers_fast(&self.app.state::<AppState>()))
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
            let worked = self.work(&mut queue);
            drop(queue);
            self.show();
            worked?;
        }
        Ok(added)
    }

    /// Queues `paths`, or, without a model installed, offers the models for them: they follow
    /// the downloads under way, or those the DJ starts for them ([`Self::download_for`]).
    pub fn separate(&self, paths: Vec<PathBuf>) -> AppResult<SeparateDto> {
        if !self.installed().is_empty() {
            self.enqueue(paths)?;
            return Ok(SeparateDto::Queued);
        }
        let downloading = lock(&self.downloads).iter().any(|d| d.running);
        {
            let mut queue = lock(&self.queue);
            if downloading {
                wait_for_models(&mut queue, &paths);
            }
            queue.offered = paths;
        }
        self.show();
        Ok(SeparateDto::NeedsModels)
    }

    /// Separates with the small models (`fast`) or the large ones from now on, downloads those
    /// missing, the tracks offered last following once they are in.
    pub fn download_for(&self, fast: bool) -> AppResult<()> {
        let state = self.app.state::<AppState>();
        let mut changed = false;
        state.update_settings(|p| {
            changed = p.library.stems_fast != fast;
            p.library.stems_fast = fast;
        });
        if changed {
            self.rechosen();
        }
        {
            let mut queue = lock(&self.queue);
            let offered = std::mem::take(&mut queue.offered);
            wait_for_models(&mut queue, &offered);
        }
        for model in MODELS.iter().filter(|m| m.fast == fast) {
            self.download_model(model.id)?;
        }
        self.release();
        Ok(())
    }

    /// The tracks waiting for the models queued, once no download is under way and a model is
    /// installed.
    fn release(&self) {
        let downloading = lock(&self.downloads).iter().any(|d| d.running);
        if downloading || self.installed().is_empty() {
            return self.show();
        }
        let awaiting = std::mem::take(&mut lock(&self.queue).awaiting);
        match self.enqueue(awaiting) {
            Ok(_) => self.show(),
            Err(e) => error!("the tracks waiting for the models are not separated: {e}"),
        }
    }

    /// Those of `paths` an installed model has stems to add to, not queued already; all of them
    /// while no model is installed (to separate once one is).
    pub fn separable(&self, paths: Vec<PathBuf>) -> Vec<PathBuf> {
        let installed = self.installed();
        if installed.is_empty() {
            return paths;
        }
        let held = lock(&self.queue).held();
        paths.into_iter().filter(|p| !held.contains(p) && !missing(&installed, p).is_empty()).collect()
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
        let worked = self.work(&mut queue);
        drop(queue);
        self.show();
        worked
    }

    /// Wakes the worker, or starts it: the caller then shows how the queue stands.
    fn work(&self, queue: &mut Queue) -> AppResult<()> {
        let running = queue.progress.running;
        queue.progress.queued = queue.len() + usize::from(running);
        if !running {
            let device = queue.progress.device.take();
            queue.progress =
                StemsProgressDto { running: true, queued: queue.len(), device, ..Default::default() };
        }
        if queue.worker.is_none() {
            let id = queue.started;
            queue.started += 1;
            let worker = Worker {
                id,
                app: self.app.clone(),
                interrupt: self.interrupt.clone(),
                models: (0..MODELS.len()).map(|m| self.model_path(m)).collect(),
                queue: self.queue.clone(),
                wake: self.wake.clone(),
            };
            std::thread::Builder::new()
                .name("prisme-stems".into())
                .spawn(move || worker.run())
                .map_err(|e| AppError::Other(e.to_string()))?;
            queue.worker = Some(id);
        }
        self.wake.notify_one();
        Ok(())
    }

    /// Empties the queue and stops the track being separated.
    pub fn cancel(&self) {
        lock(&self.queue).cancel();
        self.interrupt.store(true, Ordering::Relaxed);
        self.show();
    }

    /// The stems of `path` as they come, if it is being separated.
    fn live(&self, path: &Path) -> Option<StemTracks> {
        lock(&self.queue).live.as_ref().filter(|(p, _)| p == path).map(|(_, stems)| stems.clone())
    }

    /// The DJ chose the other models, or one was installed: the track being separated starts
    /// again with those in use now, and every later one is separated with them.
    pub fn rechosen(&self) {
        self.interrupt.store(true, Ordering::Relaxed);
    }

    /// Downloads the model `id` in the background; nothing if it is there or on its way.
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
            match result {
                Ok(()) if MODELS[model].fast == prefers_fast(&app.state::<AppState>()) => {
                    app.state::<StemsService>().rechosen()
                }
                Ok(()) => {}
                Err(e) => {
                    error!("download of the {} model failed: {e}", MODELS[model].id);
                    state[model].error = Some(e);
                }
            }
            drop(state);
            app.state::<StemsService>().release();
        });
        self.show();
        Ok(())
    }
}

/// `paths` among the tracks waiting for the models, once each.
fn wait_for_models(queue: &mut Queue, paths: &[PathBuf]) {
    for path in paths {
        if !queue.awaiting.contains(path) {
            queue.awaiting.push(path.clone());
        }
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
    let folder = target.parent().ok_or("no folder for the model")?;
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let partial = target.with_extension("part");
    let client = crate::http::client(DOWNLOAD_TIMEOUT)?;
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
            app.state::<StemsService>().show();
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
    interrupt: Arc<AtomicBool>,
    /// Where each of [`MODELS`] is (installed or not).
    models: Vec<PathBuf>,
    queue: Arc<Mutex<Queue>>,
    wake: Arc<Condvar>,
}

type BoxedSeparator = Separator<Box<dyn MaskNetwork>>;

/// A track taken from the queue.
struct Job {
    path: PathBuf,
    /// Loaded on a deck when queued.
    played: bool,
    /// In the library's queue too.
    requested: bool,
    /// The queue's cancels when it was taken.
    cancels: u64,
}

/// How a separation ended, short of an error.
enum Ended {
    /// Every stem stored.
    Through,
    /// Given way to a track loaded on a deck: back in the queue.
    GaveWay,
    /// Off the decks, and nobody asked for it otherwise.
    Dropped,
    /// Other models in use since it began: back in the queue, to start again with them.
    Rechosen,
}

impl Worker {
    fn run(self) {
        prisme_library::background_priority();
        let _stopped = Stopped { id: self.id, app: self.app.clone(), queue: self.queue.clone() };
        let mut separators: Vec<Option<BoxedSeparator>> = (0..MODELS.len()).map(|_| None).collect();
        let allowed = || gpu_allowed(&self.app.state::<AppState>());
        let open_gpu = || {
            let gpu = allowed()
                .then(|| {
                    Gpu::open(GpuOptions::default()).inspect_err(|e| warn!("separating on the CPU: {e}")).ok()
                })
                .flatten();
            self.give_up_gpu(gpu.is_none());
            gpu
        };
        let mut gpu = open_gpu();
        let mut gpu_failed = false;
        while let Some(job) = self.next() {
            // the graphics chip let or taken off since: the models open again where they may
            if allowed() != gpu.is_some() && !(gpu.is_none() && gpu_failed) {
                separators.iter_mut().for_each(|s| *s = None);
                gpu = if allowed() { open_gpu() } else { None };
            }
            let installed = self.in_use();
            // the models no longer in use give their memory back, a graphics card's above all
            for (m, separator) in separators.iter_mut().enumerate() {
                if !installed.contains(&m) {
                    *separator = None;
                }
            }
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
            let result = self.separate(&mut chosen, &job, &installed);
            lock(&self.queue).live = None;
            if !matches!(result, Ok(Ended::Through)) {
                stopped_short(&self.app, &job.path);
            }
            match result {
                Ok(Ended::Through) => {
                    self.update(|p| p.done += 1);
                    let separation = stored_separation(&job.path);
                    self.app.state::<AppState>().separated(&job.path.display().to_string(), separation);
                    attach_stems(&self.app, &job.path);
                }
                Ok(Ended::GaveWay) => {
                    info!("separation of {} gives way to a track on a deck", job.path.display());
                    lock(&self.queue).put_back(job, false);
                }
                Ok(Ended::Dropped) => info!("{} left the decks before its separation", job.path.display()),
                Ok(Ended::Rechosen) => {
                    info!("separation of {} starts again with the models chosen", job.path.display());
                    let played = job.played;
                    lock(&self.queue).put_back(job, played);
                }
                Err(StemsError::Cancelled) => info!("separation of {} cancelled", job.path.display()),
                Err(StemsError::Gpu(e)) => {
                    // out of memory, a driver reset: the GPU starts afresh, the CPU after
                    // a second failure; the track is tried again first
                    warn!("separation of {} failed on the GPU: {e}", job.path.display());
                    separators.iter_mut().for_each(|s| *s = None);
                    gpu = if gpu_failed { None } else { open_gpu() };
                    self.give_up_gpu(gpu.is_none());
                    gpu_failed = true;
                    let played = job.played;
                    lock(&self.queue).put_back(job, played);
                }
                Err(e) => {
                    warn!("{} not separated: {e}", job.path.display());
                    self.update(|p| p.failed += 1);
                }
            }
        }
    }

    /// The next track, the decks' first, and of those a playing one's: waits for one up to
    /// [`IDLE`].
    fn next(&self) -> Option<Job> {
        let playing = self.playing();
        let mut queue = lock(&self.queue);
        let job = loop {
            if let Some(job) = queue.take(&playing) {
                break job;
            }
            if queue.progress.running {
                idle(&mut queue.progress);
                drop(queue);
                self.app.state::<StemsService>().show();
                queue = lock(&self.queue);
                continue;
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

    /// The installed models the separation uses now (see [`in_use`]).
    fn in_use(&self) -> Vec<usize> {
        let installed: Vec<usize> = (0..MODELS.len()).filter(|&m| self.models[m].is_file()).collect();
        in_use(&installed, prefers_fast(&self.app.state::<AppState>()))
    }

    /// Separates the job's track with `models`, its stems reaching the decks it is loaded on
    /// as they come; other models chosen meanwhile take it over, unless a deck plays it with
    /// a part set apart ([`Self::heard_apart`]): it goes on with these until it no longer does.
    fn separate(
        &self,
        separators: &mut [&mut BoxedSeparator],
        job: &Job,
        models: &[usize],
    ) -> Result<Ended, StemsError> {
        let deck = self.on_decks(&job.path);
        let mut separation = TrackSeparation::open(
            separators,
            &job.path,
            deck.as_ref().map(|(audio, _)| &**audio),
            deck.as_ref().map(|&(_, frame)| frame),
        )?;
        self.interrupt.store(false, Ordering::Relaxed);
        let mut live: Option<Live> = None;
        while !separation.is_done() {
            if lock(&self.queue).cancelled(job) {
                return Err(StemsError::Cancelled);
            }
            if self.in_use() != models && !self.heard_apart(&job.path) {
                return Ok(Ended::Rechosen);
            }
            let at = self.on_decks(&job.path).map(|(_, frame)| frame);
            let on_decks = at.is_some();
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
            // where the deck plays when the next chunk is chosen: it may jump meanwhile
            match separation.step(separators, || self.on_decks(&job.path).map(|(_, frame)| frame)) {
                // interrupted: what for is looked at above, and the chunk taken again
                Err(StemsError::Cancelled) => {
                    self.interrupt.store(false, Ordering::Relaxed);
                    continue;
                }
                stepped => stepped?,
            }
            if let Some(live) = &mut live {
                live.publish(&separation)?;
            }
            separation.store_through()?;
            self.update(|p| p.fraction = separation.done() as f64 / separation.chunks() as f64);
        }
        Ok(Ended::Through)
    }

    /// Whether a deck plays `path` with a part of it set apart (turned down, muted, or under
    /// an effect of its own): its stems are heard, and a separation started again would take
    /// them from under it until the new one reached where it plays.
    fn heard_apart(&self, path: &Path) -> bool {
        let state = self.app.state::<AppState>();
        let mixed: Vec<(usize, bool)> = lock(&state.tracks)
            .iter()
            .enumerate()
            .filter_map(|(deck, t)| {
                t.as_ref().filter(|t| t.path == path).map(|t| (deck, t.stem_mix != StemMixDto::default()))
            })
            .collect();
        mixed.into_iter().any(|(deck, mixed)| {
            (mixed || lock(&state.decks)[deck].fx.part.is_some()) && state.deck_plays(deck)
        })
    }

    /// The tracks on the decks that play.
    fn playing(&self) -> Vec<PathBuf> {
        let state = self.app.state::<AppState>();
        let loaded: Vec<(usize, PathBuf)> = lock(&state.tracks)
            .iter()
            .enumerate()
            .filter_map(|(deck, t)| t.as_ref().map(|t| (deck, t.path.clone())))
            .collect();
        loaded.into_iter().filter(|&(deck, _)| state.deck_plays(deck)).map(|(_, path)| path).collect()
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
        let unopened: Vec<usize> = needed.iter().copied().filter(|&m| separators[m].is_none()).collect();
        let opened = std::thread::scope(|scope| {
            let opening: Vec<_> =
                unopened.iter().map(|&m| (m, scope.spawn(move || self.open_model(gpu, m)))).collect();
            opening
                .into_iter()
                .map(|(m, opening)| (m, opening.join().expect("a model opens without panicking")))
                .collect::<Vec<_>>()
        });
        for (m, opened) in opened {
            let (network, device, fast) = opened?;
            info!("separating with the {} model on {device}", MODELS[m].id);
            self.update(|p| p.device = Some(device));
            let separator = Separator::new(network);
            separators[m] = Some(if fast { separator } else { separator.overlapping_at_most(SLOW_OVERLAP) });
        }
        Ok(())
    }

    /// Model `m` as a network, on `gpu` if it can, where it runs, and whether that is fast
    /// (on the matrix units of a graphics card).
    fn open_model(
        &self,
        gpu: Option<&Arc<Gpu>>,
        m: usize,
    ) -> Result<(Box<dyn MaskNetwork>, String, bool), StemsError> {
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
                let (device, fast) = (network.adapter().to_owned(), network.uses_matrix_units());
                (Box::new(network), device, fast)
            }
            None => {
                let network =
                    CpuNetwork::with_pool(weights, background_pool()?).interrupted_by(self.interrupt.clone());
                (Box::new(network), "CPU".to_owned(), false)
            }
        })
    }

    fn update(&self, f: impl FnOnce(&mut StemsProgressDto)) {
        f(&mut lock(&self.queue).progress);
        self.app.state::<StemsService>().show();
    }

    /// The graphics card given up for the processor, or separating again: shown as it changes.
    fn give_up_gpu(&self, gave_up: bool) {
        if GPU_GAVE_UP.swap(gave_up, Ordering::Relaxed) != gave_up {
            self.app.state::<StemsService>().show();
        }
    }
}

/// The stems of a track being separated, given to the decks it is loaded on: those still
/// separated fill in as they come, the others are read from their files.
struct Live {
    /// The stems being separated, by their index in `TrackSeparation::stems`.
    filling: Vec<Option<Arc<TrackAudio>>>,
    /// The frames of each given to the decks so far: islands in order, apart.
    published: Vec<Vec<Range<usize>>>,
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
        let mut live = Self { published: vec![Vec::new(); filling.len()], filling };
        live.publish(separation)?;
        let state = app.state::<AppState>();
        let decks: Vec<(usize, u64)> = lock(&state.tracks)
            .iter()
            .enumerate()
            .filter_map(|(deck, t)| t.as_ref().filter(|t| t.path == path).map(|t| (deck, t.load_id)))
            .collect();
        for (deck, load_id) in decks {
            give(app, deck, load_id, &stems, frames, rate);
        }
        Ok(live)
    }

    /// Fills in what is final of the stems since last time.
    fn publish(&mut self, separation: &TrackSeparation) -> Result<(), StemsError> {
        let stems = separation.stems().into_iter().zip(&self.filling).zip(&mut self.published);
        for (index, (((_, finished), audio), published)) in stems.enumerate() {
            let Some(audio) = audio else { continue };
            for island in &finished {
                for part in outside(island, published) {
                    audio.publish(part.start, &separation.read(index, part)?);
                }
            }
            *published = finished;
        }
        Ok(())
    }
}

/// The frames of `island` outside the islands `known`, in order and apart.
fn outside(island: &Range<usize>, known: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut parts = Vec::new();
    let mut from = island.start;
    for k in known.iter().filter(|k| k.start < island.end && k.end > island.start) {
        if from < k.start {
            parts.push(from..k.start);
        }
        from = from.max(k.end);
    }
    if from < island.end {
        parts.push(from..island.end);
    }
    parts
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
        {
            let mut queue = lock(&self.queue);
            if queue.worker != Some(self.id) {
                return;
            }
            queue.worker = None;
            queue.live = None;
            idle(&mut queue.progress);
        }
        self.app.state::<StemsService>().show();
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

/// Gives the stored stems of `path` to the decks it is loaded on (once fully decoded:
/// otherwise the end of its loading does it), but for those that got them as they came,
/// which are told they have them whole.
pub fn attach_stems(app: &AppHandle, path: &Path) {
    let stored = stored_separation(path);
    let state = app.state::<AppState>();
    let mut audio = None;
    // stems filled where they played: their decks' tracks show them whole
    let mut whole = false;
    let loads: Vec<(usize, u64)> = lock(&state.tracks)
        .iter()
        .enumerate()
        .filter_map(|(deck, t)| {
            let t = t.as_ref().filter(|t| t.path == path && t.audio.is_complete())?;
            if plays_whole(&t.stems, stored) {
                whole = true;
                return None;
            }
            audio.get_or_insert_with(|| t.audio.clone());
            Some((deck, t.load_id))
        })
        .collect();
    if let Some(audio) = audio {
        give_stems(app, &loads, path, &audio);
    }
    if whole {
        state.moved(Section::Tracks);
    }
}

/// The decks playing `path` with the stems of a separation stopped short keep what it made,
/// where it made it: their track offers its separation again. A stem it made nothing of
/// leaves the deck, all of them without the vocals (the drums and bass count only with them).
fn stopped_short(app: &AppHandle, path: &Path) {
    let state = app.state::<AppState>();
    let decks: Vec<(usize, StemTracks)> = lock(&state.tracks)
        .iter_mut()
        .enumerate()
        .filter_map(|(deck, t)| {
            let t = t
                .as_mut()
                .filter(|t| t.path == path && t.stems.iter().flatten().any(|s| !s.is_complete()))?;
            let vocals = t.stems[Stem::Vocals as usize].as_ref().is_some_and(|s| !s.is_blank());
            t.stems = t.stems.clone().map(|stem| stem.filter(|s| vocals && !s.is_blank()));
            Some((deck, t.stems.clone()))
        })
        .collect();
    for (deck, stems) in decks {
        let _ = state.send_if_running(Command::SetStems { deck, stems });
    }
}

/// Whether `stems` are whole and go as far as `separation`.
fn plays_whole(stems: &StemTracks, separation: SeparationDto) -> bool {
    SeparationDto::of(|s| stems[s as usize].is_some()) == separation
        && stems.iter().flatten().all(|s| s.is_complete())
}

/// Gives the decks playing `loads` of `path` (its decoded `audio`) its stems: as they come if
/// it is being separated, else those stored, read once for them all.
pub fn give_stems(app: &AppHandle, loads: &[(usize, u64)], path: &Path, audio: &TrackAudio) {
    if loads.is_empty() {
        return;
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
    for &(deck, load_id) in loads {
        give(app, deck, load_id, &stems, frames, rate);
    }
}

/// Gives `stems` to the deck playing `load_id` if they fit its track and separate it at all.
fn give(app: &AppHandle, deck: usize, load_id: u64, stems: &StemTracks, frames: usize, rate: u32) {
    if DeckStemsDto::of(stems).separation == SeparationDto::None {
        return;
    }
    let state = app.state::<AppState>();
    let fits = state.change_loaded(deck, load_id, |t| {
        let fits = t.audio.frames() == frames && t.audio.sample_rate() == rate;
        if fits {
            t.stems = stems.clone();
        }
        fits
    });
    if fits == Some(true) {
        let _ = state.send_if_running(Command::SetStems { deck, stems: stems.clone() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VOCALS: usize = 0;
    const DRUMS_BASS: usize = 1;
    const FOUR_FAST: usize = 2;

    /// The models as they stand: installed, or downloading so far.
    fn shown(models: [(bool, Option<f64>); 3]) -> Shown {
        let models = (0..MODELS.len())
            .map(|m| ModelStatusDto {
                id: MODELS[m].id,
                installed: models[m].0,
                downloading: models[m].1,
                error: None,
                bytes: MODELS[m].bytes,
            })
            .collect();
        Shown { models, ..Default::default() }
    }

    #[test]
    fn the_settings_show_the_card_once_found_and_the_processor_once_it_gave_up() {
        let mut preferences = preferences(false);
        preferences.library.stems_gpu = true;
        let looking = Shown::default();
        let found = Shown { gpu: Some(true), ..Shown::default() };
        assert_ne!(looking, found, "the card found moves what is shown");
        let dto = found.dto(&preferences);
        assert_eq!((dto.gpu, dto.on_gpu, dto.on_load), (Some(true), true, true));
        let gave_up = Shown { gave_up: true, ..found.clone() };
        assert_ne!(gave_up, found);
        let dto = gave_up.dto(&preferences);
        assert_eq!((dto.on_gpu, dto.on_load), (false, false), "separating on the processor");
    }

    #[test]
    fn the_tracks_queued_stay_the_same_set_while_the_queue_holds_the_same() {
        let queued = Queued::default().again(HashSet::from([PathBuf::from("a.mp3")]));
        assert_eq!(queued.again(HashSet::from([PathBuf::from("a.mp3")])), queued);
        assert_ne!(queued.again(HashSet::new()), queued);
    }

    fn preferences(fast: bool) -> Preferences {
        let mut preferences = Preferences::default();
        preferences.library.stems_fast = fast;
        preferences
    }

    #[test]
    fn the_models_chosen_reach_the_vocals_then_every_stem() {
        let none = (false, None);
        let installed = (true, None);
        assert_eq!(shown([none, installed, installed]).reach(false), SeparationDto::None, "no vocals");
        assert_eq!(shown([installed, none, none]).reach(false), SeparationDto::Vocals);
        assert_eq!(shown([installed, installed, none]).reach(false), SeparationDto::Full);
        assert_eq!(shown([installed, installed, none]).reach(true), SeparationDto::None, "never the others");
        assert_eq!(shown([none, none, installed]).reach(true), SeparationDto::Full);
    }

    #[test]
    fn the_settings_list_the_models_chosen_the_drums_and_bass_after_the_vocals() {
        let rows = |shown: &Shown, fast| {
            let dto = shown.dto(&preferences(fast));
            dto.models.iter().map(|r| (r.status.id, r.ready)).collect::<Vec<_>>()
        };
        let nothing = shown([(false, None); 3]);
        assert_eq!(rows(&nothing, false), [("vocals", true), ("drums-bass", false)]);
        assert_eq!(rows(&nothing, true), [("four-fast", true)]);
        let vocals = shown([(true, None), (false, None), (false, None)]);
        assert_eq!(rows(&vocals, false), [("vocals", true), ("drums-bass", true)]);
    }

    #[test]
    fn each_choice_tells_what_it_has_left_to_download() {
        let vocals = shown([(true, None), (false, None), (false, None)]).dto(&preferences(false));
        assert_eq!(vocals.quality.bytes_left, Some(MODELS[DRUMS_BASS].bytes));
        assert_eq!(vocals.fast.bytes_left, Some(MODELS[FOUR_FAST].bytes));
        let all = shown([(true, None); 3]).dto(&preferences(false));
        assert_eq!((all.quality.bytes_left, all.fast.bytes_left), (None, None));
    }

    #[test]
    fn the_downloads_and_the_queue_go_by_their_share() {
        let mut downloading = shown([(false, Some(0.5)), (false, Some(0.0)), (false, None)]);
        let (vocals, drums_bass) = (MODELS[VOCALS].bytes as f64, MODELS[DRUMS_BASS].bytes as f64);
        let expected = vocals * 0.5 / (vocals + drums_bass);
        assert!((downloading.downloading().unwrap() - expected).abs() < 1e-9);
        assert_eq!(shown([(true, None); 3]).downloading(), None);

        let track = PathBuf::from("a.mp3");
        downloading.awaiting = vec![track.clone()];
        assert!((downloading.separating(&track).unwrap() - expected).abs() < 1e-9, "waits for the models");
        downloading.progress = StemsProgressDto {
            running: true,
            queued: 3,
            done: 1,
            current: Some("b.mp3".into()),
            fraction: 0.5,
            ..Default::default()
        };
        assert_eq!(downloading.separating(Path::new("b.mp3")), Some(0.5));
        assert_eq!(downloading.separating(Path::new("c.mp3")), None);
        assert!((downloading.overall() - 1.5 / 4.0).abs() < 1e-9);
        assert_eq!(downloading.current(), Some((PathBuf::from("b.mp3"), 50)));
    }

    #[test]
    fn tracks_offered_wait_once_each() {
        let mut queue = Queue::default();
        let (a, b) = (PathBuf::from("a.mp3"), PathBuf::from("b.mp3"));
        wait_for_models(&mut queue, &[a.clone(), b.clone()]);
        wait_for_models(&mut queue, std::slice::from_ref(&b));
        assert_eq!(queue.awaiting, [a, b]);
    }

    #[test]
    fn the_stems_come_from_the_models_chosen() {
        assert_eq!(in_use(&[VOCALS, DRUMS_BASS, FOUR_FAST], false), [VOCALS, DRUMS_BASS]);
        assert_eq!(in_use(&[VOCALS, DRUMS_BASS, FOUR_FAST], true), [FOUR_FAST]);
        assert_eq!(in_use(&[VOCALS, FOUR_FAST], false), [VOCALS]);
    }

    /// The large models never stand in for the small one, which a computer without a graphics
    /// card worth it was chosen for, nor the other way round.
    #[test]
    fn never_the_others_in_their_place() {
        assert!(in_use(&[VOCALS, DRUMS_BASS], true).is_empty());
        assert!(in_use(&[FOUR_FAST], false).is_empty());
    }

    /// What the worker compares, chunk after chunk, to the models a separation began with:
    /// the other choice made meanwhile, or a model of the choice installed, starts it again.
    #[test]
    fn another_choice_or_a_model_installed_meanwhile_changes_the_models_in_use() {
        let began = in_use(&[VOCALS, DRUMS_BASS, FOUR_FAST], false);
        assert_ne!(in_use(&[VOCALS, DRUMS_BASS, FOUR_FAST], true), began);
        let began = in_use(&[VOCALS], false);
        assert_ne!(in_use(&[VOCALS, DRUMS_BASS], false), began);
        assert_eq!(in_use(&[VOCALS, FOUR_FAST], false), began, "another choice's model changes nothing");
    }

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn an_island_grown_and_joined_gives_only_its_new_frames() {
        assert_eq!(outside(&(0..10), &[]), [0..10]);
        assert_eq!(outside(&(0..30), &[5..10, 20..25]), [0..5, 10..20, 25..30]);
        assert_eq!(outside(&(5..25), &[5..10, 20..25]), [10..20]);
        assert!(outside(&(5..10), &[0..10, 40..50]).is_empty());
    }

    #[test]
    fn a_cancel_stops_the_job_taken_before_it_and_no_later_one() {
        let mut queue = Queue::default();
        queue.waiting.push_back("a.mp3".into());
        let a = queue.take(&[]).unwrap();
        queue.cancel();
        queue.played.push_back("b.mp3".into());
        let b = queue.take(&[]).unwrap();
        assert!(queue.cancelled(&a));
        assert!(!queue.cancelled(&b));
        queue.put_back(a, false);
        assert_eq!(queue.len(), 0);
        queue.put_back(b, true);
        assert_eq!(queue.played, [PathBuf::from("b.mp3")]);
    }
}
