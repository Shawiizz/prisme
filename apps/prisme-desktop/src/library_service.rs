//! The analysed library inside the app: the track database, the background analysis
//! of the music folders, and the events that keep the UI up to date:
//!
//! * `track-analyzed` ([`TrackAnalyzedDto`]) for each file analysed;
//! * `library-progress` ([`ProgressDto`]) as the run advances;
//! * `music-folders-changed` ([`MusicFolderDto`] list) when a music folder is added, removed,
//!   chosen as the main one, goes away or comes back.
//!
//! The music folders are the main one and those added next to it (a USB key, a drive, a
//! sets folder). They are watched ([`FolderWatch`]): new and changed files are analysed as
//! they come. One added or chosen is watched and scanned at once; the tracks of one removed
//! or left are handled as files gone from the library.

use crate::dto::{MusicFolderDto, ProgressDto, TrackAnalyzedDto, TrackDetailsDto};
use crate::error::{AppError, AppResult};
use crate::folder_watch::FolderWatch;
use crate::state::{lock, AppState};
use crate::tag_writer::TagWriter;
use prisme_analysis::AnalysisOptions;
use prisme_library::{
    AnalysisListener, AnalysisProgress, BackgroundAnalyzer, FileStamp, LibraryDb, TrackRecord,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tracing::{error, info, warn};

pub const TRACK_ANALYZED_EVENT: &str = "track-analyzed";
pub const PROGRESS_EVENT: &str = "library-progress";
pub const FOLDERS_CHANGED_EVENT: &str = "music-folders-changed";
/// How often the music folders are looked for: one away may be back, one there may be gone.
const WATCH_RETRY: Duration = Duration::from_secs(30);

pub struct LibraryService {
    pub db: Arc<LibraryDb>,
    app: AppHandle,
    /// The music folders: the main one first, then those added, in the order they were.
    roots: Mutex<Vec<PathBuf>>,
    /// Told when the music folders change.
    changed: Condvar,
    /// Music folders left whose tracks were not handled yet (a run stopped before).
    left: Arc<Mutex<Vec<PathBuf>>>,
    analyzer: Mutex<Option<BackgroundAnalyzer>>,
    progress: Arc<Mutex<AnalysisProgress>>,
    /// The database could not be opened: the library lives in memory, lost at exit.
    in_memory: bool,
}

impl LibraryService {
    /// Opens the database in the app data folder. If it cannot be opened the app still
    /// works, with a database in memory (analyses are then redone at each start). `root` is
    /// the main music folder, `added` those added next to it.
    pub fn open(app: &AppHandle, data_dir: Option<&Path>, root: PathBuf, added: Vec<PathBuf>) -> Self {
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
            roots: Mutex::new(std::iter::once(root).chain(added).collect()),
            changed: Condvar::new(),
            left: Arc::default(),
            analyzer: Mutex::new(None),
            progress: Arc::new(Mutex::new(AnalysisProgress::default())),
            in_memory,
        }
    }

    /// Whether the library lives in memory only, its database not opened.
    pub fn in_memory(&self) -> bool {
        self.in_memory
    }

    /// The main music folder.
    pub fn root(&self) -> PathBuf {
        lock(&self.roots)[0].clone()
    }

    /// The music folders, the main one first.
    pub fn roots(&self) -> Vec<PathBuf> {
        lock(&self.roots).clone()
    }

    /// The music folders as the interface shows them, the main one first.
    pub fn folders(&self) -> Vec<MusicFolderDto> {
        self.roots()
            .into_iter()
            .enumerate()
            .map(|(i, root)| MusicFolderDto {
                present: root.is_dir(),
                main: i == 0,
                path: root.display().to_string(),
            })
            .collect()
    }

    /// Makes `root` the main music folder, watched and scanned at once; `false` if it is
    /// already. A folder added that it is or holds is no longer listed apart.
    pub fn move_to(&self, root: PathBuf) -> bool {
        let before = {
            let mut roots = lock(&self.roots);
            if roots[0] == root {
                return false;
            }
            let before = std::mem::replace(&mut roots[0], root.clone());
            let count = roots.len();
            let added = roots.split_off(1);
            roots.extend(added.into_iter().filter(|folder| !within(folder, &root)));
            if roots.len() != count {
                self.save(&roots);
            }
            before
        };
        info!("the music folder is now {}", root.display());
        lock(&self.left).retain(|dir| *dir != root);
        self.leave(before);
        true
    }

    /// Adds `folder` to the music folders, watched and scanned at once. Refused on iOS, which
    /// lets Prisme read its own documents only, and for a folder that is one of them, is in
    /// one, or holds one.
    pub fn add(&self, folder: &Path) -> AppResult<()> {
        if cfg!(target_os = "ios") {
            return Err(AppError::FoldersFixed);
        }
        let folder: PathBuf = folder.components().collect();
        if !folder.is_absolute() || !folder.is_dir() {
            return Err(AppError::FolderNotFound(folder.display().to_string()));
        }
        {
            let mut roots = lock(&self.roots);
            may_join(&roots, &folder)?;
            roots.push(folder.clone());
            self.save(&roots);
        }
        info!("{} is a music folder too", folder.display());
        lock(&self.left).retain(|dir| *dir != folder);
        self.changed();
        Ok(())
    }

    /// Removes the music folder `folder` added before, its files left as they are: its tracks
    /// are handled as files gone from the library. The main one stays.
    pub fn remove(&self, folder: &Path) -> AppResult<()> {
        {
            let mut roots = lock(&self.roots);
            if roots[0] == folder {
                return Err(AppError::MainFolderStays);
            }
            let Some(at) = roots.iter().position(|root| root == folder) else { return Ok(()) };
            roots.remove(at);
            self.save(&roots);
        }
        info!("{} is no longer a music folder", folder.display());
        self.leave(folder.to_path_buf());
        Ok(())
    }

    /// Handles the tracks of `folder`, a music folder no longer, as files gone from the
    /// library.
    fn leave(&self, folder: PathBuf) {
        {
            let mut left = lock(&self.left);
            if !left.contains(&folder) {
                left.push(folder);
            }
        }
        self.changed();
    }

    /// Tells the watch and the interface that the music folders changed, and scans them.
    fn changed(&self) {
        self.changed.notify_all();
        self.announce_folders();
        self.start();
    }

    /// Saves the music folders added next to the main one, `roots[0]`.
    fn save(&self, roots: &[PathBuf]) {
        let added = roots[1..].iter().map(|folder| folder.display().to_string()).collect();
        self.app.state::<AppState>().update_preferences(|p| p.library.folders = added);
    }

    fn announce_folders(&self) {
        let _ = self.app.emit(FOLDERS_CHANGED_EVENT, self.folders());
    }

    /// Waits up to `time` while the music folders stay `roots`.
    fn wait_change(&self, roots: &[PathBuf], time: Duration) {
        let current = lock(&self.roots);
        drop(self.changed.wait_timeout_while(current, time, |current| current.as_slice() == roots));
    }

    /// (Re)starts the analysis of the music folders: new and changed files only.
    pub fn start(&self) {
        let listener = Arc::new(EventListener {
            app: self.app.clone(),
            progress: self.progress.clone(),
            left: self.left.clone(),
        });
        let analyzer = BackgroundAnalyzer::leaving(
            self.db.clone(),
            self.roots(),
            lock(&self.left).clone(),
            prisme_library::default_workers(),
            AnalysisOptions::default(),
            listener,
        );
        // the previous run (if any) is stopped outside the lock
        let previous = lock(&self.analyzer).replace(analyzer);
        drop(previous);
    }

    /// Analyses the files of the music folders as they appear or change. The watch is set up
    /// on a thread of its own, the window never waiting on the network (a NAS offline). The
    /// folders are looked for every half minute: one away (a NAS offline, a drive unplugged)
    /// is watched again once back, and scanned, as it could not be meanwhile; the interface
    /// is told when one goes or comes back. Folders added or chosen are watched at once.
    pub fn watch(self: &Arc<Self>) {
        let library = self.clone();
        let spawned = std::thread::Builder::new().name("prisme-music-watch".into()).spawn(move || {
            let (mut roots, mut there) = (Vec::new(), Vec::new());
            let mut watch = Watch::default();
            loop {
                let roots_now = library.roots();
                let there_now: Vec<PathBuf> =
                    roots_now.iter().filter(|root| root.is_dir()).cloned().collect();
                let back: Vec<&PathBuf> =
                    there_now.iter().filter(|root| roots.contains(*root) && !there.contains(*root)).collect();
                for root in &back {
                    info!("the music folder {} is back", root.display());
                }
                for root in
                    there.iter().filter(|root| roots_now.contains(*root) && !there_now.contains(*root))
                {
                    warn!("the music folder {} went away", root.display());
                }
                if roots_now == roots && there_now != there {
                    library.announce_folders();
                }
                let caught_up = watch.cover(&library, &there_now);
                if !back.is_empty() || caught_up {
                    library.start();
                }
                library.wait_change(&roots_now, WATCH_RETRY);
                (roots, there) = (roots_now, there_now);
            }
        });
        if let Err(e) = spawned {
            error!("the music folders are not watched: {e}");
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

/// The watch of the music folders that are there.
#[derive(Default)]
struct Watch {
    current: Option<FolderWatch>,
    /// The folders watched.
    watched: Vec<PathBuf>,
    /// The folders that could not be watched, told once until they are.
    failed: Vec<PathBuf>,
}

impl Watch {
    /// Watches `folders`, set up again when they are not the ones watched: a folder that goes
    /// ends its watch without a word, and one that could not be watched is tried again.
    /// `true` when one that could not be watched now is: its changes meanwhile were missed.
    fn cover(&mut self, library: &Arc<LibraryService>, folders: &[PathBuf]) -> bool {
        if self.watched == folders {
            return false;
        }
        drop(self.current.take());
        self.watched.clear();
        let mut failed = Vec::new();
        match FolderWatch::start(library.app.clone(), library.clone()) {
            Ok(mut watch) => {
                for folder in folders {
                    match watch.add(folder) {
                        Ok(()) => self.watched.push(folder.clone()),
                        Err(e) => {
                            if !self.failed.contains(folder) {
                                warn!("{} is not watched until it can be: {e}", folder.display());
                            }
                            failed.push(folder.clone());
                        }
                    }
                }
                self.current = Some(watch);
            }
            Err(e) => {
                if self.failed.is_empty() {
                    warn!("the music folders are not watched until they can be: {e}");
                }
                failed = folders.to_vec();
            }
        }
        let caught_up = self.failed.iter().any(|folder| self.watched.contains(folder));
        self.failed = failed;
        caught_up
    }
}

/// Whether `folder` may join the music folders `roots`: not one of them, in one, or holding
/// one.
fn may_join(roots: &[PathBuf], folder: &Path) -> AppResult<()> {
    if let Some(root) = roots.iter().find(|root| within(folder, root)) {
        return Err(AppError::FolderAlreadyIn(root.display().to_string()));
    }
    if let Some(root) = roots.iter().find(|root| within(root, folder)) {
        return Err(AppError::FolderHoldsAnother(root.display().to_string()));
    }
    Ok(())
}

/// Whether `path` is `folder` or under it; on Windows, whatever the case of the letters.
fn within(path: &Path, folder: &Path) -> bool {
    let names = |path: &Path| -> Vec<String> {
        path.components()
            .map(|c| {
                let name = c.as_os_str().to_string_lossy();
                if cfg!(windows) {
                    name.to_lowercase()
                } else {
                    name.into_owned()
                }
            })
            .collect()
    };
    names(path).starts_with(&names(folder))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_joins_the_music_folders_unless_one_holds_the_other() {
        let roots = [PathBuf::from("C:/Users/dj/Music"), PathBuf::from("E:/Sets")];
        let code = |folder: &str| may_join(&roots, Path::new(folder)).map_err(|e| e.to_string());
        assert_eq!(code("D:/Music"), Ok(()));
        assert_eq!(code("E:/Sets 2024"), Ok(()), "a name that only starts the same");
        assert_eq!(code("E:/Sets"), Err("already in the music folder E:/Sets".into()));
        assert_eq!(
            code("C:/Users/dj/Music/House"),
            Err("already in the music folder C:/Users/dj/Music".into())
        );
        assert_eq!(code("E:/"), Err("holds the music folder E:/Sets".into()));
        assert_eq!(code("C:/Users"), Err("holds the music folder C:/Users/dj/Music".into()));
    }

    #[test]
    fn folders_compare_whatever_the_case_on_windows() {
        let (path, folder) = (Path::new("e:/sets/House"), Path::new("E:/Sets"));
        assert_eq!(within(path, folder), cfg!(windows));
        assert!(within(Path::new("E:/Sets/House"), folder));
    }
}
