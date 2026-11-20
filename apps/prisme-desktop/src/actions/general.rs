//! What the DJ sets for every deck (quantize), and the recording of the mix (see `recording`).

use super::Performing;
use crate::dto::{MixModeDto, RecordSourceDto, RecordingDto};
use crate::error::{AppError, AppResult};
use crate::library_service::LibraryService;
use crate::recording::Recorder;
use crate::state::{free_inputs, lock, AppState};
use prisme_engine::Command;
use prisme_library::LibraryDb;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

/// Where recordings go, in the documents: out of the music folder, so that they do not end up
/// in the library.
const FOLDER: &str = "Prisme Recordings";

/// Loops, jumps, cues and samples on the beat, or not; kept for the next start.
pub fn set_quantize(state: &AppState, enabled: bool) -> AppResult<()> {
    state.update_preferences(|p| p.dj.quantize = enabled);
    state.send_if_running(Command::SetQuantize { enabled })
}

/// Starts recording `source`, never the master of a software mixer not in use (silence);
/// returns the file being written. The source is kept: a recording started without asking
/// takes it again.
pub fn start_recording(
    app: &AppHandle,
    state: &AppState,
    db: Arc<LibraryDb>,
    source: RecordSourceDto,
) -> AppResult<String> {
    let mut recorder = lock(&state.recorder);
    if recorder.is_some() {
        return Err(AppError::AlreadyRecording);
    }
    if source == RecordSourceDto::Master && state.sample_rate().is_some() && !state.mixes_itself() {
        return Err(AppError::SilentMaster);
    }
    #[cfg(target_os = "android")]
    let documents = crate::folders::shared_documents();
    #[cfg(not(target_os = "android"))]
    let documents = app.path().document_dir().map_err(|e| AppError::Other(e.to_string()))?;
    let started = Recorder::start(app.clone(), state, source.into(), &documents.join(FOLDER), db)?;
    let path = started.path().display().to_string();
    *recorder = Some(started);
    state.update_preferences(|p| p.mixer.record_source = Some(source));
    Ok(path)
}

/// Ends the recording, if one runs, and writes its tracklist next to it (the end of the
/// recording first): another stop waits until it is.
pub fn stop_recording(state: &AppState) -> Option<RecordingDto> {
    let mut recorder = lock(&state.recorder);
    recorder.take().map(|recorder| recorder.stop(state))
}

/// What a recording started without asking takes (from a controller, the command palette):
/// the source recorded `last` if it is still `offered` and, for inputs, no turntable comes in
/// there (they are among the `free` ones), else the software mixer's master if it is offered;
/// none otherwise, rather than inputs that may be silent or carry a record's timecode.
fn preferred_source(
    offered: &[RecordSourceDto],
    last: Option<RecordSourceDto>,
    free: &[usize],
) -> Option<RecordSourceDto> {
    let kept = last.filter(|&last| {
        offered.contains(&last)
            && match last {
                RecordSourceDto::Master => true,
                RecordSourceDto::Input { first } => free.contains(&first),
            }
    });
    kept.or_else(|| offered.iter().copied().find(|&s| s == RecordSourceDto::Master))
}

/// What a recording started without asking takes on the running device (see
/// [`preferred_source`]): its master if the software mixer mixes, else one of its input pairs.
fn preferred_on(state: &AppState) -> Option<RecordSourceDto> {
    let info = lock(&state.session).as_ref().map(|s| s.info.clone());
    let mixer = state.preferences.read(|p| p.mixer);
    let mode = info.as_ref().map_or(mixer.mode, |info| state.routing(info).mode.into());
    let inputs = info.as_ref().map_or(0, |info| info.input_channels);
    let master = (mode == MixModeDto::Internal).then_some(RecordSourceDto::Master);
    let pairs = (0..inputs.saturating_sub(1)).step_by(2).map(|first| RecordSourceDto::Input { first });
    let offered: Vec<RecordSourceDto> = master.into_iter().chain(pairs).collect();
    let free = info.as_ref().map(free_inputs).unwrap_or_default();
    preferred_source(&offered, mixer.record_source, &free)
}

impl Performing<'_> {
    /// The recording ended if one runs, else started on what was recorded last (see
    /// [`preferred_source`]).
    pub(super) fn record(&self) -> AppResult<()> {
        if lock(&self.state.recorder).is_some() {
            stop_recording(self.state);
            return Ok(());
        }
        let source = preferred_on(self.state).ok_or(AppError::NoRecordSource)?;
        let Some(app) = self.app else { return Ok(()) };
        let Some(library) = app.try_state::<Arc<LibraryService>>() else { return Ok(()) };
        start_recording(app, self.state, library.db.clone(), source).map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::bench::Bench;

    const MASTER: RecordSourceDto = RecordSourceDto::Master;

    fn pair(first: usize) -> RecordSourceDto {
        RecordSourceDto::Input { first }
    }

    #[test]
    fn a_recording_started_without_asking_takes_the_master_when_nothing_was_recorded() {
        assert_eq!(preferred_source(&[MASTER, pair(0)], None, &[0, 1]), Some(MASTER));
    }

    #[test]
    fn it_takes_again_the_inputs_recorded_last_while_no_turntable_comes_in_there() {
        assert_eq!(preferred_source(&[MASTER, pair(0), pair(4)], Some(pair(4)), &[4, 5]), Some(pair(4)));
        assert_eq!(preferred_source(&[MASTER, pair(0)], Some(pair(0)), &[]), Some(MASTER));
    }

    #[test]
    fn it_records_nothing_with_an_external_mixer_until_its_inputs_are_chosen() {
        assert_eq!(preferred_source(&[pair(0), pair(2)], None, &[]), None);
        assert_eq!(preferred_source(&[pair(0), pair(2)], Some(MASTER), &[]), None);
        assert_eq!(preferred_source(&[pair(0)], Some(pair(6)), &[6, 7]), None);
    }

    #[test]
    fn the_record_key_says_when_there_is_nothing_it_may_take() {
        let bench = Bench::new();
        bench.state.update_preferences(|p| p.mixer.record_source = Some(pair(0)));
        // two decks on four inputs: a turntable on each pair, and an external mixer
        assert!(matches!(bench.press("general.record", 1.0), Err(AppError::NoRecordSource)));
        bench.state.update_preferences(|p| p.mixer.mode = MixModeDto::Internal);
        assert_eq!(preferred_on(&bench.state), Some(MASTER));
    }
}
