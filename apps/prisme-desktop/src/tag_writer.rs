//! Writes the tempo and key of the analysed tracks into their files' tags, so that other
//! DJ software finds them (a preference, on by default). One file at a time on its own
//! thread, woken whenever there may be something to write; the library's record of each
//! file follows the change, which is therefore not taken for a new version of the file.

use crate::error::AppResult;
use crate::library_service::LibraryService;
use crate::state::{lock, AppState};
use prisme_library::{FileStamp, LibraryDb, TrackRecord};
use prisme_media::TempoAndKey;
use std::path::Path;
use std::sync::{mpsc, Arc};
use tauri::{AppHandle, Manager};
use tracing::{error, warn};

pub struct TagWriter {
    wake: mpsc::Sender<()>,
}

impl TagWriter {
    pub fn start(app: AppHandle) -> Self {
        let (wake, woken) = mpsc::channel();
        std::thread::Builder::new()
            .name("prisme-tag-writer".into())
            .spawn(move || {
                while woken.recv().is_ok() {
                    while woken.try_recv().is_ok() {}
                    write_pending(&app);
                }
            })
            .expect("cannot start the tag writer thread");
        Self { wake }
    }

    /// Looks for tracks to write (after an analysis, a correction, a change of mind).
    pub fn wake(&self) {
        let _ = self.wake.send(());
    }
}

fn write_pending(app: &AppHandle) {
    let library = app.state::<Arc<LibraryService>>();
    let records = match library.db.tracks_to_tag() {
        Ok(records) => records,
        Err(e) => return error!("tracks to tag not read: {e}"),
    };
    for record in records {
        if !wanted(app) {
            return;
        }
        let path = Path::new(&record.path);
        // a deck reads the file as it decodes it: a later wake writes it
        if decoding(app, path) {
            continue;
        }
        if let Err(e) = write(&library.db, path, &record) {
            warn!("tempo and key not written into {}: {e}", path.display());
        }
    }
}

fn write(db: &LibraryDb, path: &Path, record: &TrackRecord) -> AppResult<()> {
    let before = FileStamp::of(path)?;
    // changed since it was analysed: the analysis comes first
    if before != record.version.stamp {
        return Ok(());
    }
    let analysis = record.effective_analysis();
    let values = TempoAndKey { bpm: analysis.tempo.map(|t| t.bpm), key: analysis.key.map(|k| k.key.name()) };
    if let Err(e) = prisme_media::write_tempo_and_key(path, values) {
        // not tried again until the track changes
        db.tags_written(path, before, before)?;
        return Err(e.into());
    }
    db.tags_written(path, before, FileStamp::of(path)?)?;
    Ok(())
}

fn wanted(app: &AppHandle) -> bool {
    app.state::<AppState>().preferences.read(|p| p.library.write_tags)
}

fn decoding(app: &AppHandle, path: &Path) -> bool {
    lock(&app.state::<AppState>().tracks).iter().flatten().any(|t| t.path == path && !t.audio.is_complete())
}
