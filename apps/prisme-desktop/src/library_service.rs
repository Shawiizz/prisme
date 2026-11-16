//! The analysed library inside the app: the track database, the background analysis
//! of the music folder, and the events that keep the UI up to date:
//!
//! * `track-analyzed` ([`TrackAnalyzedDto`]) for each file analysed;
//! * `library-progress` ([`ProgressDto`]) as the run advances.
//!
//! The music folder is watched ([`FolderWatch`]): new and changed files are analysed as
//! they come. Another one chosen is watched and scanned at once; the tracks of the one left
//! are handled as files gone from the library.

use crate::dto::{ProgressDto, TrackAnalyzedDto, TrackDetailsDto};
use crate::folder_watch::FolderWatch;
use crate::state::{lock, AppState};
use crate::tag_writer::TagWriter;
use prisme_analysis::AnalysisOptions;
use prisme_library::{
    AnalysisListener, AnalysisProgress, BackgroundAnalyzer, FileStamp, LibraryDb, TrackRecord,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use tauri::{AppHandle, Emitter, Manager};
use tracing::{error, info, warn};

pub const TRACK_ANALYZED_EVENT: &str = "track-analyzed";
pub const PROGRESS_EVENT: &str = "library-progress";
/// How often a music folder that cannot be watched is tried again.
const WATCH_RETRY: std::time::Duration = std::time::Duration::from_secs(30);

pub struct LibraryService {
    pub db: Arc<LibraryDb>,
    app: AppHandle,
    root: Mutex<PathBuf>,
    /// Told when the music folder changes.
    moved: Condvar,
    /// Music folders left whose tracks were not handled yet (a run stopped before).
    left: Arc<Mutex<Vec<PathBuf>>>,
    analyzer: Mutex<Option<BackgroundAnalyzer>>,
    watch: Mutex<Option<FolderWatch>>,
    progress: Arc<Mutex<AnalysisProgress>>,
    /// The database could not be opened: the library lives in memory, lost at exit.
    in_memory: bool,
}

impl LibraryService {
    /// Opens the database in the app data folder. If it cannot be opened the app still
    /// works, with a database in memory (analyses are then redone at each start).
    pub fn open(app: &AppHandle, data_dir: Option<&Path>, root: PathBuf) -> Self {
        let opened = data_dir
            .ok_or_else(|| "no data folder".to_string())
            .and_then(|dir| LibraryDb::open(&dir.join("library.sqlite")).map_err(|e| e.to_string()));
        let in_memory = opened.is_err();
        let db = opened.unwrap_or_else(|e| {
            error!("library database unavailable ({e}), keeping it in memory");
            LibraryDb::open_in_memory().expect("in-memory database")
        });
        Self {
            db: Arc::new(db),
            app: app.clone(),
            root: Mutex::new(root),
            moved: Condvar::new(),
            left: Arc::default(),
            analyzer: Mutex::new(None),
            watch: Mutex::new(None),
            progress: Arc::new(Mutex::new(AnalysisProgress::default())),
            in_memory,
        }
    }

    /// Whether the library lives in memory only, its database not opened.
    pub fn in_memory(&self) -> bool {
        self.in_memory
    }

    /// The music folder.
    pub fn root(&self) -> PathBuf {
        lock(&self.root).clone()
    }

    /// Makes `root` the music folder, watched and scanned at once; `false` if it is already.
    pub fn move_to(&self, root: PathBuf) -> bool {
        let before = std::mem::replace(&mut *lock(&self.root), root.clone());
        if before == root {
            return false;
        }
        info!("the music folder is now {}", root.display());
        {
            let mut left = lock(&self.left);
            left.retain(|dir| *dir != root);
            left.push(before);
        }
        self.moved.notify_all();
        self.start();
        true
    }

    /// Waits up to `time` while `root` stays the music folder; `false` if it changed.
    fn stays(&self, root: &Path, time: std::time::Duration) -> bool {
        let current = lock(&self.root);
        let (current, _) = self
            .moved
            .wait_timeout_while(current, time, |current| current == root)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *current == root
    }

    /// (Re)starts the analysis of the music folder: new and changed files only.
    pub fn start(&self) {
        let listener = Arc::new(EventListener {
            app: self.app.clone(),
            progress: self.progress.clone(),
            left: self.left.clone(),
        });
        let analyzer = BackgroundAnalyzer::leaving(
            self.db.clone(),
            self.root(),
            lock(&self.left).clone(),
            prisme_library::default_workers(),
            AnalysisOptions::default(),
            listener,
        );
        // the previous run (if any) is stopped outside the lock
        let previous = lock(&self.analyzer).replace(analyzer);
        drop(previous);
    }

    /// Analyses the files of the music folder as they appear or change. The watch is set up
    /// on a thread of its own, the window never waiting on the network (a NAS offline). A
    /// folder away (a NAS offline, a drive unplugged) is looked for every half minute; once
    /// back, it is watched again and scanned, as it could not be meanwhile. Another folder
    /// chosen is watched at once.
    pub fn watch(self: &Arc<Self>) {
        let library = self.clone();
        let spawned = std::thread::Builder::new().name("prisme-music-watch".into()).spawn(move || {
            let mut away = false;
            loop {
                let root = library.root();
                match FolderWatch::start(library.app.clone(), library.clone(), &root) {
                    Ok(watch) => {
                        *lock(&library.watch) = Some(watch);
                        if away {
                            info!("the music folder is back");
                            library.start();
                        }
                        // the watch of a folder that goes ends without a word
                        let mut stays = true;
                        while stays && root.exists() {
                            stays = library.stays(&root, WATCH_RETRY);
                        }
                        let ended = lock(&library.watch).take();
                        drop(ended);
                        if !stays {
                            away = false;
                            continue;
                        }
                        warn!("the music folder went away");
                    }
                    Err(e) if !away => warn!("the music folder is not watched until it can be: {e}"),
                    Err(_) => {}
                }
                away = library.stays(&root, WATCH_RETRY);
            }
        });
        if let Err(e) = spawned {
            error!("the music folder is not watched: {e}");
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
    left: Arc<Mutex<Vec<PathBuf>>>,
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

    fn left(&self, folders: &[PathBuf]) {
        lock(&self.left).retain(|dir| !folders.contains(dir));
    }

    fn holds_back(&self) -> bool {
        self.app.try_state::<AppState>().is_some_and(|state| {
            state.preferences.read(|p| p.library.pause_while_playing) && state.is_playing()
        })
    }
}

impl From<&TrackRecord> for TrackAnalyzedDto {
    fn from(r: &TrackRecord) -> Self {
        Self { path: r.path.clone(), details: TrackDetailsDto::from(r) }
    }
}
