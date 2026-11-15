//! The preview: a track of the library heard in the headphones before it goes on a deck (the
//! engine's preview). Its file is read on a worker thread while it plays, a start a third in
//! waiting for the audio to get there. Its waveform, from the library or computed once the
//! file is read (and kept there for the decks), is drawn under the library (`preview-update`
//! says when it comes).

use crate::dto::{waveform_bytes, PreviewDto};
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
}

impl Current {
    fn give_up(&self) {
        self.wanted.store(false, Ordering::Relaxed);
    }
}

impl Previews {
    /// Previews `path` in place of the track previewed; `None` if another preview, or a stop,
    /// was asked for while its file opened. Reads from the disk: off the commands' threads.
    pub fn start(&self, app: &AppHandle, path: PathBuf) -> AppResult<Option<PreviewDto>> {
        let id = self.asked.fetch_add(1, Ordering::SeqCst) + 1;
        let state = app.state::<AppState>();
        state.preview_output()?;
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
        {
            let mut current = lock(&self.current);
            if self.asked.load(Ordering::SeqCst) != id {
                return Ok(None);
            }
            let start_s = audio.duration_s() * START;
            state.send(Command::Preview { id, track: audio, start_s })?;
            let waveform_missing = waveform.is_none();
            let next = Current { id, path: path.clone(), wanted: wanted.clone(), waveform };
            if let Some(old) = current.replace(next) {
                old.give_up();
            }
            let app = app.clone();
            std::thread::Builder::new()
                .name("prisme-preview".into())
                .spawn(move || read_rest(&app, id, loading, &path, stamp, waveform_missing, &wanted))?;
        }
        Ok(Some(dto))
    }

    /// Stops the preview, and one whose file is still opening.
    pub fn stop(&self, state: &AppState) -> AppResult<()> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        if let Some(old) = lock(&self.current).take() {
            old.give_up();
        }
        state.send_if_running(Command::StopPreview)
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
        waveform.map(waveform_bytes).unwrap_or_default()
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
