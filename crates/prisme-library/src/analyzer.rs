//! Background analysis of the music folders.
//!
//! A coordinator thread scans the folders, follows the stored files that moved and forgets
//! the ones that are gone, keeps the files whose stored results are missing or stale, and
//! hands them to a few worker threads. Every thread runs in the OS "background" mode
//! (low CPU and disk priority) so that playback, which runs at high priority, is never
//! disturbed. Files are decoded chunk by chunk ([`prisme_analysis::analyze_file`]), so
//! each worker holds a few MB, not a track.

use crate::db::{path_key, FileStamp, LibraryDb, RecordVersion, TrackRecord};
use crate::identity::{adopt_unless, drive_is_there, drive_of, Search};
use crate::metadata::read_metadata;
use crate::scan::{is_online_only, scan_folders, Scan, ScannedFile};
use prisme_analysis::AnalysisOptions;
use prisme_media::MediaError;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;
use tracing::error;

/// How often a run held back looks whether it may go on.
const HELD_BACK_POLL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AnalysisProgress {
    /// Files to analyse in this run (files already up to date are not counted).
    pub total: usize,
    /// Files finished, including the failed ones.
    pub done: usize,
    pub failed: usize,
    /// False once the run is over (finished or cancelled).
    pub running: bool,
    /// Held back by the listener: no file is started.
    pub paused: bool,
}

/// Receives the results as they come, from the analysis threads.
pub trait AnalysisListener: Send + Sync + 'static {
    fn track_analyzed(&self, record: &TrackRecord);
    fn progress(&self, progress: AnalysisProgress);

    /// Asked before each file: whether the analysis holds back for now (a deck plays, and
    /// the processor and the disk are left to it).
    fn holds_back(&self) -> bool {
        false
    }

    /// The tracks of `folders`, music folders no longer, were followed into the music
    /// folders or forgotten (see [`BackgroundAnalyzer::leaving`]).
    fn left(&self, _folders: &[PathBuf]) {}
}

pub struct BackgroundAnalyzer {
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl BackgroundAnalyzer {
    /// Starts analysing the music folders `roots` with `workers` threads.
    pub fn start(
        db: Arc<LibraryDb>,
        roots: Vec<PathBuf>,
        workers: usize,
        options: AnalysisOptions,
        listener: Arc<dyn AnalysisListener>,
    ) -> Self {
        Self::leaving(db, roots, Vec::new(), workers, options, listener)
    }

    /// [`BackgroundAnalyzer::start`] once the music folders `left` are no longer among
    /// `roots`: their tracks are handled as files gone from the music folders, found again
    /// in `roots` when they were moved or copied there, else forgotten.
    pub fn leaving(
        db: Arc<LibraryDb>,
        roots: Vec<PathBuf>,
        left: Vec<PathBuf>,
        workers: usize,
        options: AnalysisOptions,
        listener: Arc<dyn AnalysisListener>,
    ) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let thread = {
            let cancel = cancel.clone();
            std::thread::Builder::new()
                .name("prisme-library".into())
                .spawn(move || {
                    let folders = Folders { roots: &roots, left: &left };
                    run(&db, folders, workers.max(1), &options, listener.as_ref(), &cancel)
                })
                .expect("cannot spawn the library thread")
        };
        Self { cancel, thread: Some(thread) }
    }

    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(|t| t.is_finished())
    }

    /// Waits for the run to complete.
    pub fn wait(mut self) {
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for BackgroundAnalyzer {
    /// Stops after the files being analysed (a few seconds at most).
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Number of analysis threads that leaves room for everything else: half the cores,
/// at most 4.
pub fn default_workers() -> usize {
    std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).clamp(1, 4))
}

/// The music folders, and those they no longer are.
#[derive(Clone, Copy)]
struct Folders<'a> {
    roots: &'a [PathBuf],
    left: &'a [PathBuf],
}

impl Folders<'_> {
    /// Whether `path` is under one of the music folders.
    fn hold(&self, path: &Path) -> bool {
        self.roots.iter().any(|root| path.starts_with(root))
    }
}

fn run(
    db: &LibraryDb,
    folders: Folders,
    workers: usize,
    options: &AnalysisOptions,
    listener: &dyn AnalysisListener,
    cancel: &AtomicBool,
) {
    background_priority();
    listener.progress(AnalysisProgress { running: true, ..Default::default() });

    let mut stored = db.versions().unwrap_or_default();
    let found = scan_folders(folders.roots, cancel);
    for copy in &found.tag_copies {
        if let Err(e) = std::fs::remove_file(copy) {
            error!("{} left behind by a tag write, not removed: {e}", copy.display());
        }
    }
    if !cancel.load(Ordering::Relaxed) {
        follow_moves(db, folders, &mut stored, &found, listener);
        if !folders.left.is_empty() {
            listener.left(folders.left);
        }
    }
    let unreadable = found.unreadable;
    let todo: Vec<ScannedFile> = found
        .files
        .into_iter()
        .filter(|f| stored.get(&path_key(&f.path)).is_none_or(|v| !v.is_current(f.stamp)))
        .collect();

    let total = todo.len();
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);
    let paused = AtomicBool::new(false);
    let report = |running: bool| {
        listener.progress(AnalysisProgress {
            total,
            done: done.load(Ordering::Relaxed),
            failed: failed.load(Ordering::Relaxed),
            running,
            paused: running && paused.load(Ordering::Relaxed),
        });
    };
    report(true);

    std::thread::scope(|s| {
        for _ in 0..workers.min(total) {
            s.spawn(|| {
                background_priority();
                while !cancel.load(Ordering::Relaxed) && next.load(Ordering::Relaxed) < total {
                    // told once, by the worker that sees it first
                    let held_back = listener.holds_back();
                    if paused.swap(held_back, Ordering::Relaxed) != held_back {
                        report(true);
                    }
                    if held_back {
                        std::thread::sleep(HELD_BACK_POLL);
                        continue;
                    }
                    let Some(file) = todo.get(next.fetch_add(1, Ordering::Relaxed)) else {
                        break;
                    };
                    // a file never seen there may be one known elsewhere: renamed and tagged
                    // again, or moved in from outside the folder; its audio is read once, for
                    // this and to be known again later
                    let audio_id = audio_id(&file.path);
                    let search = audio_id.map_or(Search::Stamp, Search::AudioId);
                    let may_be_back = |old: &Path| unreadable.iter().any(|dir| old.starts_with(dir));
                    if let Err(e) = adopt_unless(db, &file.path, file.stamp, search, may_be_back) {
                        error!("{} not looked for elsewhere: {e}", file.path.display());
                    }
                    let Some((record, cover)) = process(db, file, options) else {
                        failed.fetch_add(1, Ordering::Relaxed);
                        done.fetch_add(1, Ordering::Relaxed);
                        report(true);
                        continue;
                    };
                    if record.error.is_some() {
                        failed.fetch_add(1, Ordering::Relaxed);
                    }
                    let saved = db.save(&record).and_then(|()| match &cover {
                        Some(jpeg) => db.save_cover(&file.path, jpeg),
                        None => Ok(()),
                    });
                    if let Err(e) = saved {
                        error!("analysis of {} not saved: {e}", file.path.display());
                    }
                    if let Some(id) = audio_id.filter(|_| record.error.is_none()) {
                        keep_audio_id(db, &file.path, id);
                    }
                    listener.track_analyzed(&record);
                    done.fetch_add(1, Ordering::Relaxed);
                    report(true);
                }
            });
        }
    });

    // the tracks stored before their audio had an identity, or loaded from outside the
    // folder: read while no deck plays, but for those kept online, here or wherever they were
    // loaded from (reading would download them); a drive that is away is asked once, not for
    // each of its files
    let mut drives: HashMap<PathBuf, bool> = HashMap::new();
    for path in db.paths_without_audio_id().unwrap_or_default() {
        while listener.holds_back() && !cancel.load(Ordering::Relaxed) {
            if !paused.swap(true, Ordering::Relaxed) {
                report(true);
            }
            std::thread::sleep(HELD_BACK_POLL);
        }
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if paused.swap(false, Ordering::Relaxed) {
            report(true);
        }
        let file = Path::new(&path);
        let there = *drives.entry(drive_of(file).0).or_insert_with(|| drive_is_there(file));
        if there && !is_online_only(file) {
            remember_audio(db, file);
        }
    }

    // a run stopped for another one says nothing more: what it would say comes after the
    // other's first words, and would tell the analysis over while it goes on
    if !cancel.load(Ordering::Relaxed) {
        report(false);
    }
}

/// Stores the identity of the audio of `path`, if the file is there and can be read.
pub fn remember_audio(db: &LibraryDb, path: &Path) {
    if let Some(id) = audio_id(path) {
        keep_audio_id(db, path, id);
    }
}

/// The identity of the audio of `path`, if it can be read: a file that makes its reader panic
/// has none.
fn audio_id(path: &Path) -> Option<u64> {
    std::panic::catch_unwind(|| prisme_media::audio_id(path)).ok()?.ok()
}

fn keep_audio_id(db: &LibraryDb, path: &Path, id: u64) {
    if let Err(e) = db.set_audio_id(path, id) {
        error!("identity of {} not saved: {e}", path.display());
    }
}

/// Follows the stored files under the music folders that the scan did not find, and those of
/// the folders left: one that reappears elsewhere with the same size and modification time
/// (moved, renamed, or copied into a music folder) keeps all it had; the others are
/// forgotten, but for their cues and corrections, which a file with their audio takes as it
/// is analysed (see [`adopt_unless`]). Those under a folder the scan could not list (a drive
/// unplugged) are kept as they are. `stored` is updated to match.
fn follow_moves(
    db: &LibraryDb,
    folders: Folders,
    stored: &mut HashMap<String, RecordVersion>,
    found: &Scan,
    listener: &dyn AnalysisListener,
) {
    let present: HashSet<String> = found
        .files
        .iter()
        .map(|f| path_key(&f.path))
        .chain(found.online_only.iter().map(|p| path_key(p)))
        .collect();
    let gone: Vec<String> = stored
        .keys()
        .filter(|p| {
            let path = Path::new(p);
            if folders.hold(path) {
                !present.contains(*p) && !found.may_hold(path)
            } else {
                folders.left.iter().any(|dir| path.starts_with(dir))
            }
        })
        .cloned()
        .collect();
    let mut arrivals: HashMap<FileStamp, Vec<&ScannedFile>> = HashMap::new();
    for file in found.files.iter().filter(|f| !stored.contains_key(&path_key(&f.path))) {
        arrivals.entry(file.stamp).or_default().push(file);
    }
    // whether what was known at `old` went to `file`
    let relocate = |old: &str, file: &ScannedFile| match db.relocate(Path::new(old), &file.path) {
        Ok(()) => {
            if let Ok(Some(record)) = db.get(&file.path) {
                listener.track_analyzed(&record);
            }
            true
        }
        Err(e) => {
            error!("{old} not moved to {}: {e}", file.path.display());
            false
        }
    };
    let mut unmatched = Vec::new();
    for old in gone {
        // two arrivals with the same stamp cannot be told apart
        let version = stored[&old];
        match arrivals.get(&version.stamp).filter(|files| files.len() == 1).map(|files| files[0]) {
            Some(file) => {
                stored.remove(&old);
                arrivals.remove(&version.stamp);
                if relocate(&old, file) {
                    stored.insert(path_key(&file.path), version);
                }
            }
            None => unmatched.push(old),
        }
    }

    for old in &unmatched {
        stored.remove(old);
    }
    if let Err(e) = db.forget(&unmatched) {
        error!("removed files not forgotten: {e}");
    }
}

/// The record of a file and its cover thumbnail, none when the file could not be read at that
/// moment (its drive unplugged, the file locked or changed meanwhile): the next run reads it
/// again. When only the tags are stale, the stored analysis is kept: reading tags takes
/// milliseconds, analysing seconds. A file that makes a decoder, the tags' reader or the
/// analysis panic is recorded as failed, and left alone until it changes, instead of stopping
/// the analysis there.
fn process(
    db: &LibraryDb,
    file: &ScannedFile,
    options: &AnalysisOptions,
) -> Option<(TrackRecord, Option<Vec<u8>>)> {
    let reusable = db
        .get(&file.path)
        .ok()
        .flatten()
        .filter(|r| r.error.is_none() && r.version.analysis_is_current(file.stamp));
    let read = std::panic::catch_unwind(|| match reusable {
        Some(record) => {
            let metadata = read_metadata(&file.path);
            Ok((record.with_tags(metadata.tags), metadata.cover))
        }
        None => analyze(file, options),
    });
    let (record, cover) = match read {
        Ok(Ok(read)) => read,
        Ok(Err(e)) if e.is_transient() => return None,
        Ok(Err(e)) => (TrackRecord::failed(&file.path, file.stamp, e.to_string()), None),
        Err(_) => (TrackRecord::failed(&file.path, file.stamp, "the file could not be read".into()), None),
    };
    if FileStamp::of(&file.path).ok() != Some(file.stamp) {
        return None;
    }
    let edits = db.edits(&file.path).unwrap_or_default();
    Some((TrackRecord { edits, ..record }, cover))
}

fn analyze(
    file: &ScannedFile,
    options: &AnalysisOptions,
) -> Result<(TrackRecord, Option<Vec<u8>>), MediaError> {
    let a = prisme_analysis::analyze_file(&file.path, options)?;
    let metadata = read_metadata(&file.path);
    let record = TrackRecord::analyzed(&file.path, file.stamp, a.duration_s, a.analysis, metadata.tags);
    Ok((record, metadata.cover))
}

/// Puts the calling thread in background mode: lowest CPU and disk priority, each system its
/// own way.
pub fn background_priority() {
    #[cfg(windows)]
    // SAFETY: changes the priority of the calling thread only; no pointers involved.
    unsafe {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
        };
        SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
    }
    // Linux: run when nothing else would (0 is the calling thread), read the disk when
    // nobody else does
    #[cfg(target_os = "linux")]
    // SAFETY: changes the scheduling of the calling thread only; the parameter outlives the
    // call.
    unsafe {
        const IOPRIO_WHO_PROCESS: libc::c_int = 1;
        const IOPRIO_CLASS_IDLE: libc::c_int = 3 << 13;
        let param = libc::sched_param { sched_priority: 0 };
        libc::sched_setscheduler(0, libc::SCHED_IDLE, &param);
        libc::syscall(libc::SYS_ioprio_set, IOPRIO_WHO_PROCESS, 0, IOPRIO_CLASS_IDLE);
    }
    // macOS: the class of work nobody waits for, on the efficiency cores and after the others
    // on the disk
    #[cfg(target_os = "macos")]
    // SAFETY: changes the class of the calling thread only.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_BACKGROUND, 0);
    }
}
