//! Background analysis of a music folder.
//!
//! A coordinator thread scans the folder, forgets the stored files it does not find any
//! more, keeps the files whose stored results are missing or stale, and hands them to a
//! few worker threads. Every thread runs in the OS "background" mode (low CPU and disk
//! priority) so that playback, which runs at high priority, is never disturbed. Files
//! are decoded chunk by chunk ([`prisme_analysis::analyze_file`]), so each worker holds
//! a few MB, not a track.

use crate::db::{path_key, LibraryDb, RecordVersion, TrackRecord};
use crate::metadata::read_metadata;
use crate::scan::{scan_folder, ScannedFile};
use prisme_analysis::AnalysisOptions;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AnalysisProgress {
    /// Files to analyse in this run (files already up to date are not counted).
    pub total: usize,
    /// Files finished, including the failed ones.
    pub done: usize,
    pub failed: usize,
    /// False once the run is over (finished or cancelled).
    pub running: bool,
}

/// Receives the results as they come, from the analysis threads.
pub trait AnalysisListener: Send + Sync + 'static {
    fn track_analyzed(&self, record: &TrackRecord);
    fn progress(&self, progress: AnalysisProgress);
}

pub struct BackgroundAnalyzer {
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl BackgroundAnalyzer {
    /// Starts analysing `root` with `workers` threads.
    pub fn start(
        db: Arc<LibraryDb>,
        root: PathBuf,
        workers: usize,
        options: AnalysisOptions,
        listener: Arc<dyn AnalysisListener>,
    ) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let thread = {
            let cancel = cancel.clone();
            std::thread::Builder::new()
                .name("prisme-library".into())
                .spawn(move || run(&db, &root, workers.max(1), &options, listener.as_ref(), &cancel))
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

fn run(
    db: &LibraryDb,
    root: &std::path::Path,
    workers: usize,
    options: &AnalysisOptions,
    listener: &dyn AnalysisListener,
    cancel: &AtomicBool,
) {
    background_priority();
    listener.progress(AnalysisProgress { running: true, ..Default::default() });

    let stored = db.versions().unwrap_or_default();
    let found = scan_folder(root, cancel);
    if !cancel.load(Ordering::Relaxed) {
        forget_missing(db, root, &stored, &found);
    }
    let todo: Vec<ScannedFile> = found
        .into_iter()
        .filter(|f| stored.get(&path_key(&f.path)).is_none_or(|v| !v.is_current(f.stamp)))
        .collect();

    let total = todo.len();
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);
    listener.progress(AnalysisProgress { total, running: true, ..Default::default() });

    std::thread::scope(|s| {
        for _ in 0..workers.min(total) {
            s.spawn(|| {
                background_priority();
                while !cancel.load(Ordering::Relaxed) {
                    let Some(file) = todo.get(next.fetch_add(1, Ordering::Relaxed)) else {
                        break;
                    };
                    let (record, cover) = process(db, file, options);
                    if record.error.is_some() {
                        failed.fetch_add(1, Ordering::Relaxed);
                    }
                    let saved = db.save(&record).and_then(|()| match &cover {
                        Some(jpeg) => db.save_cover(&file.path, jpeg),
                        None => Ok(()),
                    });
                    if let Err(e) = saved {
                        eprintln!("{e}");
                    }
                    listener.track_analyzed(&record);
                    let done = done.fetch_add(1, Ordering::Relaxed) + 1;
                    listener.progress(AnalysisProgress {
                        total,
                        done,
                        failed: failed.load(Ordering::Relaxed),
                        running: true,
                    });
                }
            });
        }
    });

    listener.progress(AnalysisProgress {
        total,
        done: done.load(Ordering::Relaxed),
        failed: failed.load(Ordering::Relaxed),
        running: false,
    });
}

/// Forgets the stored files under `root` that the scan did not find.
fn forget_missing(
    db: &LibraryDb,
    root: &Path,
    stored: &HashMap<String, RecordVersion>,
    found: &[ScannedFile],
) {
    let present: HashSet<String> = found.iter().map(|f| path_key(&f.path)).collect();
    let gone: Vec<String> =
        stored.keys().filter(|p| Path::new(p).starts_with(root) && !present.contains(*p)).cloned().collect();
    if let Err(e) = db.forget(&gone) {
        eprintln!("{e}");
    }
}

/// The record of a file and its cover thumbnail. When only the tags are stale, the
/// stored analysis is kept: reading tags takes milliseconds, analysing seconds.
fn process(db: &LibraryDb, file: &ScannedFile, options: &AnalysisOptions) -> (TrackRecord, Option<Vec<u8>>) {
    let reusable = db
        .get(&file.path)
        .ok()
        .flatten()
        .filter(|r| r.error.is_none() && r.version.analysis_is_current(file.stamp));
    match reusable {
        Some(record) => {
            let metadata = read_metadata(&file.path);
            (record.with_tags(metadata.tags), metadata.cover)
        }
        None => {
            let (record, cover) = analyze(file, options);
            let edits = db.edits(&file.path).unwrap_or_default();
            (TrackRecord { edits, ..record }, cover)
        }
    }
}

fn analyze(file: &ScannedFile, options: &AnalysisOptions) -> (TrackRecord, Option<Vec<u8>>) {
    match prisme_analysis::analyze_file(&file.path, options) {
        Ok(a) => {
            let metadata = read_metadata(&file.path);
            let record =
                TrackRecord::analyzed(&file.path, file.stamp, a.duration_s, a.analysis, metadata.tags);
            (record, metadata.cover)
        }
        Err(e) => (TrackRecord::failed(&file.path, file.stamp, e.to_string()), None),
    }
}

/// Puts the calling thread in background mode: lowest CPU, disk and memory priority.
fn background_priority() {
    #[cfg(windows)]
    // SAFETY: changes the priority of the calling thread only; no pointers involved.
    unsafe {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
        };
        SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
    }
}
