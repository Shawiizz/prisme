//! Watches the music folder. When audio files appear, change or go, the new and changed
//! ones are analysed and the UI is told which folders changed (`library-files-changed`).

use crate::library_service::LibraryService;
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
                    let mut folders = BTreeSet::new();
                    changed_folders(first, &mut folders);
                    loop {
                        match received.recv_timeout(SETTLE) {
                            Ok(event) => changed_folders(event, &mut folders),
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => return,
                        }
                    }
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

/// Adds the folders whose listing `event` changes: audio files, and sub-folders (a
/// removed path cannot be checked, so a path without extension counts as a folder).
fn changed_folders(event: notify::Result<Event>, folders: &mut BTreeSet<PathBuf>) {
    let Ok(event) = event else { return };
    if matches!(event.kind, EventKind::Access(_)) {
        return;
    }
    for path in event.paths {
        let listed = prisme_media::is_supported(&path) || path.extension().is_none() || path.is_dir();
        if let Some(parent) = path.parent().filter(|_| listed) {
            folders.insert(parent.to_path_buf());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, CreateKind, RemoveKind};

    fn event(kind: EventKind, paths: &[&str]) -> notify::Result<Event> {
        Ok(paths.iter().fold(Event::new(kind), |e, p| e.add_path(PathBuf::from(p))))
    }

    #[test]
    fn only_listed_paths_count() {
        let mut folders = BTreeSet::new();
        changed_folders(
            event(EventKind::Create(CreateKind::File), &["C:/Music/House/new.mp3"]),
            &mut folders,
        );
        changed_folders(
            event(EventKind::Create(CreateKind::File), &["C:/Music/House/set.csv"]),
            &mut folders,
        );
        changed_folders(event(EventKind::Remove(RemoveKind::Any), &["C:/Music/Old Folder"]), &mut folders);
        changed_folders(event(EventKind::Access(AccessKind::Any), &["C:/Music/Techno/a.flac"]), &mut folders);
        assert_eq!(
            folders.into_iter().collect::<Vec<_>>(),
            [PathBuf::from("C:/Music"), PathBuf::from("C:/Music/House")]
        );
    }
}
