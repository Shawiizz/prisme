//! Writes into the files' tags, so that other DJ software finds what Prisme knows: the
//! tempo and key of the analysed tracks (a preference, on by default; those another
//! program wrote are kept unless the DJ chose otherwise), and the genres given by hand. One file at a time on its own thread, so that two writes never meet;
//! the library's record of each file follows the change, which is therefore not taken for
//! a new version of the file.

use crate::error::{AppError, AppResult};
use crate::library_service::LibraryService;
use crate::state::{lock, AppState};
use prisme_library::{FileStamp, LibraryDb, TrackRecord};
use prisme_media::{Replacing, TempoAndKey};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use tauri::{AppHandle, Manager};
use tracing::{error, warn};

pub struct TagWriter {
    jobs: mpsc::Sender<Job>,
}

enum Job {
    /// Look for tempos and keys to write.
    Pending,
    /// Write `genre` into these files; the ones that could not be written are sent back.
    Genre { paths: Vec<PathBuf>, genre: Option<String>, failed: mpsc::Sender<Vec<String>> },
}

impl TagWriter {
    pub fn start(app: AppHandle) -> Self {
        let (jobs, received) = mpsc::channel();
        std::thread::Builder::new()
            .name("prisme-tag-writer".into())
            .spawn(move || {
                while let Ok(job) = received.recv() {
                    match job {
                        Job::Pending => write_pending(&app),
                        Job::Genre { paths, genre, failed } => {
                            let _ = failed.send(write_genres(&app, &paths, genre.as_deref()));
                        }
                    }
                }
            })
            .expect("cannot start the tag writer thread");
        Self { jobs }
    }

    /// Looks for tracks to write (after an analysis, a correction, a change of mind).
    pub fn wake(&self) {
        let _ = self.jobs.send(Job::Pending);
    }

    /// Writes `genre` (`None`: none) into the tags of `paths`; the answer lists the files
    /// that could not be written.
    pub fn set_genre(&self, paths: Vec<PathBuf>, genre: Option<String>) -> mpsc::Receiver<Vec<String>> {
        let (failed, answer) = mpsc::channel();
        let _ = self.jobs.send(Job::Genre { paths, genre, failed });
        answer
    }
}

fn write_genres(app: &AppHandle, paths: &[PathBuf], genre: Option<&str>) -> Vec<String> {
    let library = app.state::<Arc<LibraryService>>();
    let mut failed = Vec::new();
    for path in paths {
        let result = if decoding(app, path) {
            Err(AppError::Other("loaded on a deck: try again in a moment".into()))
        } else {
            write_genre(&library.db, path, genre)
        };
        match result {
            Ok(()) => library.announce(path),
            Err(e) => {
                warn!("genre not written into {}: {e}", path.display());
                failed.push(path.display().to_string());
            }
        }
    }
    failed
}

fn write_genre(db: &LibraryDb, path: &Path, genre: Option<&str>) -> AppResult<()> {
    let before = FileStamp::of(path)?;
    if prisme_media::write_genre(path, genre)? {
        db.genre_written(path, genre, before, FileStamp::of(path)?)?;
    }
    Ok(())
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
        let replace_all = || app.state::<AppState>().preferences.read(|p| p.library.replace_tags);
        if let Err(e) = write(&library.db, path, &record, replace_all) {
            warn!("tempo and key not written into {}: {e}", path.display());
        }
    }
}

/// Writes the tempo and key of `path` as stored now (corrected since `record` was read,
/// perhaps), over those Prisme wrote before, or over any when `replace_all` says so.
fn write(db: &LibraryDb, path: &Path, record: &TrackRecord, replace_all: impl Fn() -> bool) -> AppResult<()> {
    let before = FileStamp::of(path)?;
    // changed since it was analysed: the analysis comes first
    if before != record.version.stamp {
        return Ok(());
    }
    let Some(record) = db.get(path)? else { return Ok(()) };
    let values = tempo_and_key(&record);
    let own = db.own_tags(path)?;
    let replaced_all = replace_all();
    let replacing = if replaced_all { Replacing::Any } else { Replacing::Own(own.values()) };
    let written = match prisme_media::write_tempo_and_key(path, values, replacing) {
        Ok(written) => written,
        Err(e) => {
            // not tried again until the track changes
            db.tags_written(path, before, before)?;
            return Err(e.into());
        }
    };
    db.own_tags_written(
        path,
        TempoAndKey { bpm: values.bpm.filter(|_| written.bpm), key: values.key.filter(|_| written.key) },
    )?;
    let after = FileStamp::of(path)?;
    let asked_the_same = replace_all() == replaced_all;
    if asked_the_same && db.get(path)?.is_some_and(|now| tempo_and_key(&now) == values) {
        db.tags_written(path, before, after)?;
    } else {
        // corrected, or all tags asked to be replaced, while it was written: the next pass
        // writes it again
        db.tags_outdated(path, before, after)?;
    }
    Ok(())
}

fn tempo_and_key(record: &TrackRecord) -> TempoAndKey<'static> {
    let analysis = record.effective_analysis();
    TempoAndKey { bpm: analysis.tempo.map(|t| t.bpm), key: analysis.key.map(|k| k.key.name()) }
}

fn wanted(app: &AppHandle) -> bool {
    app.state::<AppState>().preferences.read(|p| p.library.write_tags)
}

fn decoding(app: &AppHandle, path: &Path) -> bool {
    lock(&app.state::<AppState>().tracks).iter().flatten().any(|t| t.path == path && !t.audio.is_complete())
}
