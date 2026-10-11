//! Watches the music folder. When audio files appear, change or go, the new and changed
//! ones are analysed and the UI is told which folders changed (`library-files-changed`).
//! A file whose change the library already knows of (tags Prisme wrote) is left alone.

use crate::library_service::LibraryService;
use notify::event::ModifyKind;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub const FILES_CHANGED_EVENT: &str = "library-files-changed";
/// Changes are handled once the folder has been quiet this long: copying an album is a
/// burst of events.
const SETTLE: Duration = Duration::from_millis(1500);

/// Watches while it is alive.
pub struct FolderWatch {
    _watcher: RecommendedWatcher,
}

impl FolderWatch {
    pub fn start(app: AppHandle, library: Arc<LibraryService>, root: &Path) -> notify::Result<Self> {
        let (events, received) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(events)?;
        watcher.watch(root, RecursiveMode::Recursive)?;
        std::thread::Builder::new()
            .name("prisme-folder-watch".into())
            .spawn(move || {
                while let Ok(first) = received.recv() {
                    let mut changes = Changes::default();
                    changes.add(first);
                    loop {
                        match received.recv_timeout(SETTLE) {
                            Ok(event) => changes.add(event),
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => return,
                        }
                    }
                    let folders = changes.folders(|file| library.knows_current(file));
                    if folders.is_empty() {
                        continue;
                    }
                    library.start();
                    let folders: Vec<String> = folders.iter().map(|f| f.display().to_string()).collect();
                    let _ = app.emit(FILES_CHANGED_EVENT, folders);
                }
            })
            .expect("cannot start the folder watch thread");
        Ok(Self { _watcher: watcher })
    }
}

/// What a burst of events changed.
#[derive(Default)]
struct Changes {
    /// Folders whose listing changed: audio files or sub-folders came or went.
    folders: BTreeSet<PathBuf>,
    /// Audio files whose content changed.
    modified: BTreeSet<PathBuf>,
}

impl Changes {
    fn add(&mut self, event: notify::Result<Event>) {
        let Ok(event) = event else { return };
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        for path in event.paths {
            if matches!(event.kind, EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Any))
                && prisme_media::is_supported(&path)
            {
                self.modified.insert(path);
                continue;
            }
            // a removed path cannot be checked: one without extension counts as a folder
            let listed = prisme_media::is_supported(&path) || path.extension().is_none() || path.is_dir();
            if let Some(parent) = path.parent().filter(|_| listed) {
                self.folders.insert(parent.to_path_buf());
            }
        }
    }

    /// The folders to look at again, leaving out the files whose change is `known`.
    fn folders(mut self, known: impl Fn(&Path) -> bool) -> BTreeSet<PathBuf> {
        for file in self.modified.iter().filter(|f| !known(f)) {
            if let Some(parent) = file.parent() {
                self.folders.insert(parent.to_path_buf());
            }
        }
        self.folders
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, CreateKind, DataChange, RemoveKind};

    fn event(kind: EventKind, paths: &[&str]) -> notify::Result<Event> {
        Ok(paths.iter().fold(Event::new(kind), |e, p| e.add_path(PathBuf::from(p))))
    }

    #[test]
    fn only_listed_paths_count() {
        let mut changes = Changes::default();
        changes.add(event(EventKind::Create(CreateKind::File), &["C:/Music/House/new.mp3"]));
        changes.add(event(EventKind::Create(CreateKind::File), &["C:/Music/House/set.csv"]));
        changes.add(event(EventKind::Remove(RemoveKind::Any), &["C:/Music/Old Folder"]));
        changes.add(event(EventKind::Access(AccessKind::Any), &["C:/Music/Techno/a.flac"]));
        assert_eq!(
            changes.folders(|_| false).into_iter().collect::<Vec<_>>(),
            [PathBuf::from("C:/Music"), PathBuf::from("C:/Music/House")]
        );
    }

    #[test]
    fn known_changes_are_left_alone() {
        let modified = EventKind::Modify(ModifyKind::Data(DataChange::Any));
        let mut changes = Changes::default();
        changes.add(event(modified, &["C:/Music/House/tagged.mp3", "C:/Music/Techno/edited.flac"]));
        let known = |file: &Path| file.ends_with("tagged.mp3");
        assert_eq!(
            changes.folders(known).into_iter().collect::<Vec<_>>(),
            [PathBuf::from("C:/Music/Techno")]
        );
    }
}
