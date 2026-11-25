//! Loading a track on a deck, in two steps: first what makes it playable at once (the
//! start of the audio, what the library knows of it, its cached waveform); then, on a
//! worker thread, the rest of the audio and whatever was missing: the waveform, computed
//! as the audio is decoded and drawn as it comes (`track_waveform`), and the analysis,
//! shown with the deck's track once known.

use crate::error::AppResult;
use crate::library_service::LibraryService;
use crate::preview::Previews;
use crate::state::{lock, AppState, LoadedTrack};
use crate::track_waveform::{TrackWaveform, WaveformFeed};
use prisme_analysis::{AnalysisOptions, TrackAnalysis};
use prisme_engine::{Command, StemTracks};
use prisme_library::{FileStamp, LibraryDb, Search, TrackRecord};
use prisme_media::LoadingTrack;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tracing::{error, info, warn};

/// Waveform resolution: enough for a zoomed, scrolling view.
pub const WAVEFORM_SLICES_PER_S: u32 = 150;

/// What is left to do once the track is on the deck.
pub struct Completion {
    loading: LoadingTrack,
    path: PathBuf,
    stamp: Option<FileStamp>,
    waveform_missing: bool,
    analysis_missing: bool,
}

/// Puts a track on `deck` before the audio starts (`AppState::restore` hands it to the
/// engine); its waveform and analysis follow.
pub fn preload(
    app: &AppHandle,
    state: &AppState,
    library: &LibraryService,
    deck: usize,
    path: PathBuf,
) -> AppResult<()> {
    state.deck_in_use(deck)?;
    let (track, completion) = prepare(path, &library.db)?;
    lock(&state.tracks)[deck] = Some(track);
    let app = app.clone();
    std::thread::spawn(move || completion.run(&app));
    Ok(())
}

/// Puts `path` on `deck` as soon as it can play (blocks while its start is read); its
/// waveform and analysis follow. Whether it went there: not if another load was asked for on
/// the deck meanwhile, which goes there instead. The interface hears of it with the decks'
/// tracks.
pub fn load_on_deck(app: &AppHandle, deck: usize, path: PathBuf) -> AppResult<bool> {
    let state = app.state::<AppState>();
    let library = app.state::<Arc<LibraryService>>();
    let previews = app.state::<Previews>();
    let completion = put_on_deck(&state, &library.db, Some(&previews), deck, path)?;
    Ok(completion.map(|completion| completion.run_aside(app)).is_some())
}

/// Puts `path` on `deck` as soon as it can play (blocks while its start is read), as
/// `library` knows it; the preview of the same file stops (`previews`, if there are): it is on
/// a deck now. What is left to do of it, none if another load was asked for on the deck
/// meanwhile, which goes there instead.
pub fn put_on_deck(
    state: &AppState,
    library: &LibraryDb,
    previews: Option<&Previews>,
    deck: usize,
    path: PathBuf,
) -> AppResult<Option<Completion>> {
    state.deck_in_use(deck)?;
    let turn = state.ask_load(deck);
    let (track, completion) = prepare(path, library)?;
    let commands = track.load_commands(deck, state.preferences.read(|p| p.dj.auto_gain));
    let file = track.path.clone();
    if !state.put_track(&turn, Some(track), commands)? {
        info!("{} not put on deck {deck}: another load was asked for meanwhile", file.display());
        return Ok(None);
    }
    if let Err(e) = previews.map_or(Ok(()), |previews| previews.stop_previewing(state, &file)) {
        warn!("the preview of {} goes on: {e}", file.display());
    }
    crate::actions::deck::loaded(state, deck);
    Ok(Some(completion))
}

/// The track, playable, and what is left to do ([`Completion::run`]).
fn prepare(path: PathBuf, library: &LibraryDb) -> AppResult<(LoadedTrack, Completion)> {
    let loading = LoadingTrack::open(&path)?;
    let stamp = FileStamp::of(&path).ok();
    // a file the library knew elsewhere (renamed, moved, another drive letter) has its cues
    if let Some(stamp) = stamp {
        match prisme_library::adopt(library, &path, stamp, Search::Audio) {
            Ok(Some(from)) => info!("{} was {}", path.display(), from.display()),
            Ok(None) => {}
            Err(e) => warn!("{} not looked for elsewhere: {e}", path.display()),
        }
    }
    let stored = stamp
        .and_then(|s| library.get(&path).ok().flatten().filter(|r| r.is_current(s) && r.error.is_none()));
    let waveform = match stamp.and_then(|s| library.waveform(&path, s).ok().flatten()) {
        Some(stored) => TrackWaveform::whole(stored),
        None => TrackWaveform::growing(WAVEFORM_SLICES_PER_S),
    };
    let completion = Completion {
        path: path.clone(),
        stamp,
        waveform_missing: !waveform.is_whole(),
        analysis_missing: stored.is_none(),
        loading,
    };
    let (detected, edits, tags) = match stored {
        Some(r) => (r.analysis, r.edits, r.tags),
        None => (
            TrackAnalysis::default(),
            library.edits(&path).unwrap_or_default(),
            prisme_media::read_tags(&path).map(|(tags, _)| tags).unwrap_or_default(),
        ),
    };
    let cues = library.cues(&path).unwrap_or_else(|e| {
        warn!("cues of {} not read: {e}", path.display());
        Vec::new()
    });
    let track = LoadedTrack {
        load_id: LoadedTrack::next_load_id(),
        audio: completion.loading.audio().clone(),
        path,
        waveform,
        detected,
        edits,
        tags,
        cues,
        key_shift: 0,
        stems: StemTracks::default(),
        stem_mix: Default::default(),
        loudness: None,
    };
    Ok((track, completion))
}

impl Completion {
    /// Runs on a worker thread of the application's.
    pub fn run_aside(self, app: &AppHandle) {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || self.run(&app));
    }

    /// Runs on a worker thread. The results go to the decks still playing the track's audio
    /// (its load, and the instant doubles made of it meanwhile), and nowhere once another
    /// track was loaded on them.
    pub fn run(self, app: &AppHandle) {
        let state = app.state::<AppState>();
        let library = app.state::<Arc<LibraryService>>();
        // the track on the deck first: the library's analysis leaves it the processor
        let awaited = self.analysis_missing.then(|| state.await_analysis());
        let audio = self.loading.audio().clone();
        let mut feed = self.waveform_missing.then(|| WaveformFeed::new(&audio, WAVEFORM_SLICES_PER_S));
        let (audio, decoded) = self.loading.decode_rest_while(|| {
            if let Some(feed) = &mut feed {
                feed.follow(&audio, &state);
            }
            true
        });
        if let Err(e) = &decoded {
            warn!("{} decoded up to {:.1} s: {e}", self.path.display(), audio.duration_s());
        }
        // what is made of a file read in part serves the decks, but is not kept as the file's
        let stamp = self.stamp.filter(|_| decoded.is_ok());
        if let Some(feed) = feed {
            let waveform = feed.finish(&audio, &state);
            if let Some(stamp) = stamp {
                if let Err(e) = library.db.save_waveform(&self.path, stamp, &waveform) {
                    error!("waveform of {} not saved: {e}", self.path.display());
                }
            }
        }
        crate::stems_service::give_stems(app, &state.loads_of(&audio), &self.path, &audio);
        let loudness = prisme_analysis::loudness(&audio);
        let auto_gain = state.preferences.read(|p| p.dj.auto_gain);
        for (deck, _, gain) in state.change_loads_of(&audio, |t| {
            t.loudness = loudness;
            t.level(auto_gain)
        }) {
            let _ = state.send_if_running(Command::SetLevel { deck, gain });
        }
        if self.analysis_missing {
            let analysis = prisme_analysis::analyze(&audio, &AnalysisOptions::default());
            let metadata = prisme_library::read_metadata(&self.path);
            if let Some(stamp) = stamp {
                let analyzed =
                    TrackRecord::analyzed(&self.path, stamp, audio.duration_s(), analysis, metadata.tags);
                let edits = library.db.edits(&self.path).unwrap_or_default();
                library.store(&TrackRecord { edits, ..analyzed }, metadata.cover.as_deref());
            }
            for (deck, _, grid) in state.change_loads_of(&audio, |t| {
                t.detected = analysis;
                t.grid()
            }) {
                let _ = state.send_if_running(Command::SetBeatGrid { deck, grid });
            }
        }
        drop(awaited);
        // after the analysis, which a track dropped from the file manager has to go through:
        // not both at once on the processor
        if crate::stems_service::separates_on_load(&state) {
            if let Err(e) = app
                .state::<crate::stems_service::StemsService>()
                .separate_played(&self.path, audio.duration_s())
            {
                warn!("{} not separated: {e}", self.path.display());
            }
        }
        // a track loaded from outside the music folder is told again by its audio too
        if library.db.audio_id(&self.path).is_ok_and(|id| id.is_none()) {
            prisme_library::remember_audio(&library.db, &self.path);
        }
    }
}
