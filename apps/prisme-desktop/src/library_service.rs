//! The analysed library inside the app: the track database, the background analysis
//! of the music folder, and the events that keep the UI up to date:
//!
//! * `track-analyzed` ([`TrackAnalyzedDto`]) for each file analysed;
//! * `library-progress` ([`ProgressDto`]) as the run advances.
//!
//! The music folder is watched ([`FolderWatch`]): new and changed files are analysed as
//! they come.

use crate::dto::{ProgressDto, TrackAnalyzedDto, TrackDetailsDto};
use crate::folder_watch::FolderWatch;
use crate::state::lock;
use crate::tag_writer::TagWriter;
use prisme_analysis::AnalysisOptions;
use prisme_library::{
    AnalysisListener, AnalysisProgress, BackgroundAnalyzer, FileStamp, LibraryDb, TrackRecord,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};
use tracing::{error, warn};

pub const TRACK_ANALYZED_EVENT: &str = "track-analyzed";
pub const PROGRESS_EVENT: &str = "library-progress";

pub struct LibraryService {
    pub db: Arc<LibraryDb>,
    app: AppHandle,
    root: PathBuf,
    analyzer: Mutex<Option<BackgroundAnalyzer>>,
    watch: Mutex<Option<FolderWatch>>,
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
                warn!("library database unavailable ({e}), keeping it in memory");
                LibraryDb::open_in_memory().expect("in-memory database")
            });
        Self {
            db: Arc::new(db),
            app: app.clone(),
            root,
            analyzer: Mutex::new(None),
            watch: Mutex::new(None),
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

    /// Analyses the files of the music folder as they appear or change.
    pub fn watch(self: &Arc<Self>) {
        match FolderWatch::start(self.app.clone(), self.clone(), &self.root) {
            Ok(watch) => *lock(&self.watch) = Some(watch),
            Err(e) => warn!("the music folder is not watched: {e}"),
        }
    }

    pub fn progress(&self) -> AnalysisProgress {
        *lock(&self.progress)
    }

    /// Tells the UI that what the library knows of a track changed.
    pub fn announce(&self, path: &Path) {
        match self.db.get(path) {
            Ok(Some(record)) => {
                let _ = self.app.emit(TRACK_ANALYZED_EVENT, TrackAnalyzedDto::from(&record));
            }
            Ok(None) => {}
            Err(e) => error!("{} not read from the library: {e}", path.display()),
        }
    }

    /// Stores a track analysed outside the background run (on load) and tells the UI.
    pub fn store(&self, record: &TrackRecord, cover: Option<&[u8]>) {
        let saved = self.db.save(record).and_then(|()| match cover {
            Some(jpeg) => self.db.save_cover(Path::new(&record.path), jpeg),
            None => Ok(()),
        });
        if let Err(e) = saved {
            error!("analysis of {} not saved: {e}", record.path);
        }
        let _ = self.app.emit(TRACK_ANALYZED_EVENT, TrackAnalyzedDto::from(record));
        self.write_tags_soon();
    }

    /// A track's tempo or key may have to be written into its file (see `tag_writer`).
    pub fn write_tags_soon(&self) {
        self.app.state::<TagWriter>().wake();
    }

    /// Whether the library describes the current version of `path` (a change made by
    /// Prisme itself, such as tags it wrote).
    pub fn knows_current(&self, path: &Path) -> bool {
        let Ok(stamp) = FileStamp::of(path) else { return false };
        self.db.get(path).ok().flatten().is_some_and(|r| r.is_current(stamp))
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
        if !progress.running {
            self.app.state::<TagWriter>().wake();
        }
    }
}

impl From<&TrackRecord> for TrackAnalyzedDto {
    fn from(r: &TrackRecord) -> Self {
        Self { path: r.path.clone(), details: TrackDetailsDto::from(r) }
    }
}
