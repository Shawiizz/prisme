use crate::audio_host;
use crate::dto::{AudioInfo, AudioSettings, BackendDto, DeviceDto, TimecodeFormatDto};
use crate::error::AppResult;
use crate::state::{lock, AppState};
use prisme_audio_io::{Backend, DeviceKind};
use prisme_engine::Command;
use tauri::State;
use tracing::error;

/// Backends this computer offers (ASIO only with an ASIO driver installed).
#[tauri::command]
pub fn audio_backends() -> Vec<BackendDto> {
    Backend::available().into_iter().map(BackendDto::from).collect()
}

#[tauri::command]
pub fn list_devices(backend: BackendDto) -> AppResult<Vec<DeviceDto>> {
    Ok(prisme_audio_io::list_devices(backend.into())?
        .into_iter()
        .map(|d| DeviceDto {
            kind: match d.kind {
                DeviceKind::Input => "input",
                DeviceKind::Output => "output",
            },
            name: d.name,
            channels: d.channels,
            sample_rate: d.sample_rate,
            buffer_range: d.buffer_range,
            is_default: d.is_default,
        })
        .collect())
}

#[tauri::command]
pub fn timecode_formats() -> Vec<TimecodeFormatDto> {
    prisme_timecode::FORMATS.iter().map(|f| TimecodeFormatDto { id: f.id, name: f.name }).collect()
}

/// The audio settings the user chose (they may differ from the running ones if the
/// chosen device was missing at startup).
#[tauri::command]
pub fn get_audio_settings(state: State<'_, AppState>) -> AudioSettings {
    state.preferences.get().audio
}

#[tauri::command]
pub fn audio_info(state: State<'_, AppState>) -> Option<AudioInfo> {
    lock(&state.session).as_ref().map(|s| s.info.clone())
}

/// Restarts the audio with new settings and keeps them for the next start.
#[tauri::command]
pub fn apply_audio_settings(state: State<'_, AppState>, settings: AudioSettings) -> AppResult<AudioInfo> {
    let info = start_audio(&state, &settings)?;
    state.update_preferences(|p| p.audio = settings);
    Ok(info)
}

/// (Re)starts the audio and restores the decks where they were, playing if they were.
pub fn start_audio(state: &AppState, settings: &AudioSettings) -> AppResult<AudioInfo> {
    let _starting = lock(&state.audio_start);
    start(state, settings)
}

/// Starts the audio with `settings` unless it runs (started by the user meanwhile, on other
/// devices): the watchdog's way to bring it back. `None` if it ran.
pub fn start_audio_unless_running(
    state: &AppState,
    settings: &AudioSettings,
) -> AppResult<Option<AudioInfo>> {
    let _starting = lock(&state.audio_start);
    if lock(&state.session).as_ref().is_some_and(|s| !s.device_lost()) {
        return Ok(None);
    }
    start(state, settings).map(Some)
}

fn start(state: &AppState, settings: &AudioSettings) -> AppResult<AudioInfo> {
    let transport = state.transport_snapshot();
    // the device may not open twice; closed outside the lock, as it waits for the audio thread
    let previous = lock(&state.session).take();
    drop(previous);
    if settings.simulate {
        // the simulated needle starts 10 s into the record: start the track there
        let mut decks = lock(&state.decks);
        decks[0].vinyl_enabled = true;
        decks[0].lead_in_s = prisme_timecode::testing::SimulatedTurntable::START_S;
    }
    let mut session = audio_host::start(settings)?;
    state.restore(&mut session);
    for (deck, (seconds, playing)) in transport.into_iter().enumerate() {
        let _ = session.controller.send(Command::Seek { deck, seconds });
        let _ = session.controller.send(Command::SetPlaying { deck, playing });
    }
    let info = session.info.clone();
    // held from before the session is in place: a recording started on it meanwhile would
    // have its writer, on the live engine, waited for by the resume
    let mut recorder = lock(&state.recorder);
    *lock(&state.session) = Some(session);
    // a recording goes on, in a file of its own
    if let Some(Err(e)) = recorder.as_mut().map(|r| r.resume(state)) {
        error!("recording ended, not resumed: {e}");
        if let Err(e) = recorder.take().map_or(Ok(()), |r| r.stop(state).map(drop)) {
            error!("recording not ended: {e}");
        }
    }
    Ok(info)
}
