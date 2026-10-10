//! The analysed library inside the app: the track database, the background analysis
//! of the music folder, and the events that keep the UI up to date:
//!
//! * `track-analyzed` ([`TrackAnalyzedDto`]) for each file analysed;
//! * `library-progress` ([`ProgressDto`]) as the run advances.

use crate::dto::{ProgressDto, TrackAnalysisDto, TrackAnalyzedDto};
use crate::state::lock;
use prisme_analysis::AnalysisOptions;
use prisme_library::{AnalysisListener, AnalysisProgress, BackgroundAnalyzer, LibraryDb, TrackRecord};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

pub const TRACK_ANALYZED_EVENT: &str = "track-analyzed";
pub const PROGRESS_EVENT: &str = "library-progress";

pub struct LibraryService {
    pub db: Arc<LibraryDb>,
    app: AppHandle,
    root: PathBuf,
    analyzer: Mutex<Option<BackgroundAnalyzer>>,
    progress: Arc<Mutex<AnalysisProgress>>,
}

impl LibraryService {
    /// Opens the database in the app data folder. If it cannot be opened the app still
    /// works, with a database in memory (analyses are then redone at each start).
    pub fn open(app: &AppHandle, data_dir: Option<&Path>, root: PathBuf) -> Self {
        let db = data_dir
            .ok_or_else(|| "no data folder".to_string())
            .and_then(|dir| LibraryDb::open(&dir.join("library.sqlite")).map_err(|e| e.to_string()))
            .unwrap_or_else(|e| {
                eprintln!("library database unavailable ({e}), keeping it in memory");
                LibraryDb::open_in_memory().expect("in-memory database")
            });
        Self {
            db: Arc::new(db),
            app: app.clone(),
            root,
            analyzer: Mutex::new(None),
            progress: Arc::new(Mutex::new(AnalysisProgress::default())),
        }
    }

    /// (Re)starts the analysis of the music folder: new and changed files only.
    pub fn start(&self) {
        let listener = Arc::new(EventListener { app: self.app.clone(), progress: self.progress.clone() });
        let analyzer = BackgroundAnalyzer::start(
            self.db.clone(),
            self.root.clone(),
            prisme_library::default_workers(),
            AnalysisOptions::default(),
            listener,
        );
        // the previous run (if any) is stopped outside the lock
        let previous = lock(&self.analyzer).replace(analyzer);
        drop(previous);
    }

    pub fn progress(&self) -> AnalysisProgress {
        *lock(&self.progress)
    }

    /// Tells the UI about a track analysed outside the background run (on load).
    pub fn announce(&self, record: &TrackRecord) {
        let _ = self.app.emit(TRACK_ANALYZED_EVENT, TrackAnalyzedDto::from(record));
    }
}

struct EventListener {
    app: AppHandle,
    progress: Arc<Mutex<AnalysisProgress>>,
}

impl AnalysisListener for EventListener {
    fn track_analyzed(&self, record: &TrackRecord) {
        let _ = self.app.emit(TRACK_ANALYZED_EVENT, TrackAnalyzedDto::from(record));
    }

    fn progress(&self, progress: AnalysisProgress) {
        *lock(&self.progress) = progress;
        let _ = self.app.emit(PROGRESS_EVENT, ProgressDto::from(progress));
    }
}

impl From<&TrackRecord> for TrackAnalyzedDto {
    fn from(r: &TrackRecord) -> Self {
        Self { path: r.path.clone(), analysis: TrackAnalysisDto::from(r) }
    }
}
