//! The preview: a track of the library heard in the headphones before it goes on a deck (the
//! engine's preview), as the actions ask for it (`library.preview`) or the screen for one of
//! its tracks. Its file is read on a worker thread while it plays, a start a third in waiting
//! for the audio to get there. The interface shows what is previewed from `AppState::preview`;
//! the waveform, from the library or computed once the file is read (and kept there for the
//! decks), is drawn under the library (`preview-update` says when it comes).

use crate::dto::{waveform_bytes, PreviewDto, PreviewStateDto};
use crate::error::AppResult;
use crate::folders;
use crate::library_service::LibraryService;
use crate::state::{lock, AppState};
use crate::track_loading::WAVEFORM_SLICES_PER_S;
use prisme_analysis::Waveform;
use prisme_engine::Command;
use prisme_library::FileStamp;
use prisme_media::LoadingTrack;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};
use tracing::{error, warn};

pub const PREVIEW_UPDATE_EVENT: &str = "preview-update";
/// Where a preview starts, as a part of the track: about where the music is.
const START: f64 = 1.0 / 3.0;

#[derive(Default)]
pub struct Previews {
    /// The last preview asked for (or stop): one still opening gives way to it.
    asked: AtomicU64,
    current: Mutex<Option<Current>>,
}

struct Current {
    id: u64,
    path: PathBuf,
    /// Cleared when it is no longer wanted: the worker reading its file gives up.
    wanted: Arc<AtomicBool>,
    waveform: Option<Waveform>,
    /// The engine reported it playing: once it no longer does, it stopped it.
    reported: bool,
}

impl Current {
    fn give_up(&self) {
        self.wanted.store(false, Ordering::Relaxed);
    }
}

impl Previews {
    /// Previews `path`, or stops it if it is the one previewed. Reads from the disk: off the
    /// threads of the controllers and the commands.
    pub fn toggle(&self, app: &AppHandle, path: PathBuf) -> AppResult<()> {
        let state = app.state::<AppState>();
        if self.previewing(&state, &path) {
            self.stop(&state)
        } else {
            self.start(app, path)
        }
    }

    /// Whether `path` is previewed: its file opens for it, or the engine plays it (not once it
    /// stopped it at the end of the track).
    fn previewing(&self, state: &AppState, path: &Path) -> bool {
        let playing = state.engine_status().ok().and_then(|s| s.preview).map(|p| p.id);
        let current = lock(&self.current);
        let shown = current.as_ref().is_some_and(|c| c.path == path && Some(c.id) == playing);
        shown || lock(&state.preview).opening == Some(path.display().to_string())
    }

    /// Previews `path` in place of the track previewed, unless another preview, or a stop, is
    /// asked for while its file opens; one that cannot be read stops the preview.
    fn start(&self, app: &AppHandle, path: PathBuf) -> AppResult<()> {
        let id = self.asked.fetch_add(1, Ordering::SeqCst) + 1;
        let state = app.state::<AppState>();
        state.preview_output()?;
        lock(&state.preview).opening = Some(path.display().to_string());
        let opened = self.open(app, &state, id, path);
        if self.asked.load(Ordering::SeqCst) != id {
            return opened;
        }
        match opened {
            Ok(()) => {
                lock(&state.preview).opening = None;
                Ok(())
            }
            Err(e) => self.stop(&state).and(Err(e)),
        }
    }

    /// Opens `path` and has the engine preview it as preview `id`, if it is still the one asked
    /// for.
    fn open(&self, app: &AppHandle, state: &AppState, id: u64, path: PathBuf) -> AppResult<()> {
        let library = app.state::<Arc<LibraryService>>();
        let loading = LoadingTrack::open(&path)?;
        let stamp = FileStamp::of(&path).ok();
        let stored = stamp.and_then(|s| library.db.get(&path).ok().flatten().filter(|r| r.is_current(s)));
        let tags = match stored {
            Some(record) => record.tags,
            None => prisme_media::read_tags(&path).map(|(tags, _)| tags).unwrap_or_default(),
        };
        let waveform = stamp.and_then(|s| library.db.waveform(&path, s).ok().flatten());
        let audio = loading.audio().clone();
        let dto = PreviewDto {
            id,
            path: path.display().to_string(),
            title: tags.title.unwrap_or_else(|| folders::title_from_path(&path)),
            artist: tags.artist,
            duration_s: audio.duration_s(),
            waveform_ready: waveform.is_some(),
        };
        let wanted = Arc::new(AtomicBool::new(true));
        let mut current = lock(&self.current);
        if self.asked.load(Ordering::SeqCst) != id {
            return Ok(());
        }
        let start_s = audio.duration_s() * START;
        state.send(Command::Preview { id, track: audio, start_s })?;
        let waveform_missing = waveform.is_none();
        let next = Current { id, path: path.clone(), wanted: wanted.clone(), waveform, reported: false };
        if let Some(old) = current.replace(next) {
            old.give_up();
        }
        lock(&state.preview).track = Some(dto);
        let app = app.clone();
        std::thread::Builder::new()
            .name("prisme-preview".into())
            .spawn(move || read_rest(&app, id, loading, &path, stamp, waveform_missing, &wanted))?;
        Ok(())
    }

    /// Stops the preview, and one whose file is still opening.
    pub fn stop(&self, state: &AppState) -> AppResult<()> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        if let Some(old) = lock(&self.current).take() {
            old.give_up();
        }
        *lock(&state.preview) = PreviewStateDto::default();
        state.send_if_running(Command::StopPreview)
    }

    /// Follows the engine previewing `playing` (its id): a preview it reported then stopped (at
    /// the end of its track, as it went on a deck, as the audio restarted) is over, a file
    /// opening meanwhile for the next still shown.
    pub fn follow(&self, state: &AppState, playing: Option<u64>) {
        let mut current = lock(&self.current);
        let Some(shown) = current.as_mut() else { return };
        if playing == Some(shown.id) {
            shown.reported = true;
        } else if shown.reported {
            if let Some(old) = current.take() {
                old.give_up();
            }
            lock(&state.preview).track = None;
        }
    }

    /// Stops the preview if it plays `path`: the track went on a deck.
    pub fn stop_previewing(&self, state: &AppState, path: &Path) -> AppResult<()> {
        let previewing = lock(&self.current).as_ref().is_some_and(|c| c.path == path);
        if previewing {
            self.stop(state)?;
        }
        Ok(())
    }

    /// The waveform of preview `id`, in binary (`dto::waveform_bytes`); empty when it is not
    /// known yet or another track is previewed.
    pub fn waveform(&self, id: u64) -> Vec<u8> {
        let current = lock(&self.current);
        let waveform = current.as_ref().filter(|c| c.id == id).and_then(|c| c.waveform.as_ref());
        waveform.map(|w| waveform_bytes(w, w.len())).unwrap_or_default()
    }
}

/// Reads the rest of the file while the preview is wanted, then computes its waveform if the
/// library had none.
fn read_rest(
    app: &AppHandle,
    id: u64,
    loading: LoadingTrack,
    path: &Path,
    stamp: Option<FileStamp>,
    waveform_missing: bool,
    wanted: &AtomicBool,
) {
    let (audio, decoded) = loading.decode_rest_while(|| wanted.load(Ordering::Relaxed));
    if !wanted.load(Ordering::Relaxed) {
        return;
    }
    if let Err(e) = decoded {
        warn!("{} previewed up to {:.1} s: {e}", path.display(), audio.duration_s());
    }
    if !waveform_missing {
        return;
    }
    let waveform = prisme_analysis::compute_waveform(&audio, WAVEFORM_SLICES_PER_S);
    if let Some(stamp) = stamp {
        let library = app.state::<Arc<LibraryService>>();
        if let Err(e) = library.db.save_waveform(path, stamp, &waveform) {
            error!("waveform of {} not saved: {e}", path.display());
        }
    }
    let previews = app.state::<Previews>();
    if let Some(current) = lock(&previews.current).as_mut().filter(|c| c.id == id) {
        current.waveform = Some(waveform);
    } else {
        return;
    }
    let _ = app.emit(PREVIEW_UPDATE_EVENT, id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::PreviewDto;
    use prisme_session::JsonFile;

    #[test]
    fn a_preview_the_engine_reported_then_stopped_is_over() {
        let state = AppState::new(JsonFile::in_memory(Default::default()));
        let previews = Previews::default();
        let wanted = Arc::new(AtomicBool::new(true));
        let current =
            Current { id: 3, path: "a.mp3".into(), wanted: wanted.clone(), waveform: None, reported: false };
        *lock(&previews.current) = Some(current);
        let dto = PreviewDto {
            id: 3,
            path: "a.mp3".into(),
            title: "A".into(),
            artist: None,
            duration_s: 200.0,
            waveform_ready: false,
        };
        lock(&state.preview).track = Some(dto);
        previews.follow(&state, None);
        assert!(lock(&state.preview).track.is_some(), "not started yet");
        previews.follow(&state, Some(3));
        // the next opening as it ends
        lock(&state.preview).opening = Some("b.mp3".into());
        previews.follow(&state, None);
        assert!(lock(&state.preview).track.is_none());
        assert_eq!(lock(&state.preview).opening.as_deref(), Some("b.mp3"), "the next still opening");
        assert!(!wanted.load(Ordering::Relaxed), "its file no longer read");
    }
}
