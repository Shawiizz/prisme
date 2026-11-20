//! The analysed library inside the app: the track database, the background analysis
//! of the music folders, what of it the library as browsed takes at once (`actions::browse`: a
//! file analysed, a scan ended, the music folders changed), and what the interface shows of the
//! folders and the analysis (`AppState::folders`, see [`LibraryService::show`]).
//!
//! The music folders are the main one and those added next to it (a USB key, a drive, a
//! sets folder). They are watched ([`FolderWatch`]): new and changed files are analysed as
//! they come. One added or chosen is watched and scanned at once; the tracks of one removed
//! or left are handled as files gone from the library.

use crate::actions::browse;
use crate::dto::MusicFolderDto;
use crate::error::{AppError, AppResult};
use crate::folder_watch::FolderWatch;
use crate::state::{lock, AppState};
use crate::tag_writer::TagWriter;
use prisme_analysis::AnalysisOptions;
use prisme_library::{
    within, AnalysisListener, AnalysisProgress, BackgroundAnalyzer, FileStamp, LibraryDb, TrackRecord,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tracing::{error, info, warn};

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
    /// The run of the analysis started last: each look through the folders the next.
    scan: Arc<AtomicU64>,
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
            scan: Arc::default(),
            in_memory,
        }
    }

    /// Shows the interface the music folders, each asked whether it is there.
    pub fn show_folders(&self) {
        let folders = self.folders();
        let state = self.app.state::<AppState>();
        let mut shown = lock(&state.folders);
        (shown.folders, shown.in_memory) = (folders, self.in_memory);
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
        self.app.state::<AppState>().update_settings(|p| p.library.folders = added);
    }

    fn announce_folders(&self) {
        self.show_folders();
        browse::folders_changed(&self.app.state::<AppState>(), &self.db, self.roots());
    }

    /// The library as it is browsed, read whole, then the folder shown last opened: at start,
    /// off the main thread.
    pub fn browse(self: &Arc<Self>) {
        let library = self.clone();
        let spawned = std::thread::Builder::new().name("prisme-library-view".into()).spawn(move || {
            browse::start(&library.app.state::<AppState>(), &library.db, library.roots());
        });
        if let Err(e) = spawned {
            error!("the library is not shown: {e}");
        }
    }

    /// The crates, the sets or the prepare list changed (an import): their lists read again.
    pub fn lists_changed(&self) {
        if let Err(e) = browse::lists(&self.app.state::<AppState>(), &self.db) {
            error!("the lists of the library not read: {e}");
        }
    }

    /// Waits up to `time` while the music folders stay `roots`.
    fn wait_change(&self, roots: &[PathBuf], time: Duration) {
        let current = lock(&self.roots);
        drop(self.changed.wait_timeout_while(current, time, |current| current.as_slice() == roots));
    }

    /// (Re)starts the analysis of the music folders: new and changed files only. The run it
    /// starts.
    pub fn start(&self) -> u64 {
        let scan = self.scan.fetch_add(1, Ordering::SeqCst) + 1;
        let listener = Arc::new(EventListener {
            app: self.app.clone(),
            db: self.db.clone(),
            left: self.left.clone(),
            scans: self.scan.clone(),
            scan,
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
        scan
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

    /// What the library knows of a track changed: its rows show it.
    pub fn announce(&self, path: &Path) {
        browse::reread(&self.app.state::<AppState>(), &self.db, [path]);
    }

    /// Stores a track analysed outside the background run (on load): its rows show it.
    pub fn store(&self, record: &TrackRecord, cover: Option<&[u8]>) {
        let saved = self.db.save(record).and_then(|()| match cover {
            Some(jpeg) => self.db.save_cover(Path::new(&record.path), jpeg),
            None => Ok(()),
        });
        if let Err(e) = saved {
            error!("analysis of {} not saved: {e}", record.path);
        }
        browse::analysed(&self.app.state::<AppState>(), record.clone());
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
    db: Arc<LibraryDb>,
    left: Arc<Mutex<Vec<PathBuf>>>,
    /// The run started last, and this one's: a run stopped for another is not shown.
    scans: Arc<AtomicU64>,
    scan: u64,
}

impl AnalysisListener for EventListener {
    fn track_analyzed(&self, record: &TrackRecord) {
        if let Some(state) = self.app.try_state::<AppState>() {
            browse::analysed(&state, record.clone());
        }
    }

    /// A run that ended may have found files gone or moved: the library as browsed is read
    /// whole again.
    fn progress(&self, progress: AnalysisProgress) {
        if self.scans.load(Ordering::SeqCst) == self.scan {
            if let Some(state) = self.app.try_state::<AppState>() {
                let mut shown = lock(&state.folders);
                (shown.progress, shown.scan) = (progress.into(), self.scan);
            }
        }
        if !progress.running {
            self.app.state::<TagWriter>().wake();
            if let Some(state) = self.app.try_state::<AppState>() {
                browse::reload(&state, &self.db);
            }
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
}
