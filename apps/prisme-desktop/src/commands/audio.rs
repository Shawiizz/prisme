use crate::audio_host;
use crate::commands::blocking;
use crate::dto::{
    AudioInfo, AudioSettings, BackendChoiceDto, BackendDto, BufferSizeDto, DeviceDto, LatencyDto,
    TimecodeFormatDto,
};
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState};
use prisme_audio_io::{latency, AudioError, Backend, DeviceKind};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use tracing::error;

/// The interface went out of sight, or came back.
#[tauri::command]
pub fn set_hidden(state: State<'_, AppState>, hidden: bool) {
    state.hidden.store(hidden, std::sync::atomic::Ordering::Relaxed);
}

/// Backends this computer offers (ASIO only with an ASIO driver installed).
#[tauri::command]
pub fn audio_backends() -> Vec<BackendChoiceDto> {
    Backend::available()
        .into_iter()
        .map(BackendDto::from)
        .map(|backend| BackendChoiceDto { backend, one_device: one_device(backend) })
        .collect()
}

/// Whether `backend` reaches one device for the outputs and the inputs.
fn one_device(backend: BackendDto) -> bool {
    matches!(backend, BackendDto::Asio | BackendDto::Usb)
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
            buffer_sizes: BufferSizeDto::offered(d.buffer_range, d.sample_rate),
            is_default: d.is_default,
        })
        .collect())
}

/// The DJ chose not to use the sound card `name` just plugged in: it is not offered again.
#[tauri::command]
pub fn decline_card(state: State<'_, AppState>, name: String) {
    let base = prisme_audio_io::base_device_name(&name);
    state.update_preferences(|p| {
        if !p.declined_cards.contains(&base) {
            p.declined_cards.push(base);
        }
    });
}

#[tauri::command]
pub fn timecode_formats() -> Vec<TimecodeFormatDto> {
    prisme_timecode::FORMATS.iter().map(TimecodeFormatDto::from).collect()
}

/// The audio settings the user chose (they may differ from the running ones if the
/// chosen device was missing at startup).
#[tauri::command]
pub fn get_audio_settings(state: State<'_, AppState>) -> AudioSettings {
    state.preferences.get().audio
}

#[tauri::command]
pub fn audio_info(state: State<'_, AppState>) -> Option<AudioInfo> {
    let saved = state.preferences.read(|p| p.audio.clone());
    lock(&state.session).as_ref().map(|s| AudioInfo {
        missing: (!s.settings.same_devices(&saved)).then(|| saved.output_device.clone().unwrap_or_default()),
        ..s.info.clone()
    })
}

/// Restarts the audio with new settings and keeps them for the next start. Off the commands'
/// threads: a USB card may wait for the DJ to let Prisme open it.
#[tauri::command]
pub async fn apply_audio_settings(app: AppHandle, settings: AudioSettings) -> AppResult<AudioInfo> {
    blocking(move || {
        let state = app.state::<AppState>();
        // kept as they start: no other start comes between
        let _starting = lock(&state.audio_start);
        let saved = state.preferences.read(|p| p.audio.clone());
        let info = match start(&state, &settings) {
            // devices missing since the start stay chosen, the defaults playing meanwhile
            Err(AppError::Audio(AudioError::DeviceNotFound(_))) if settings.same_devices(&saved) => {
                start(&state, &settings.on_default_devices())?
            }
            started => started?,
        };
        state.update_preferences(|p| p.audio = settings);
        Ok(info)
    })
    .await
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

/// Starts the audio with `settings` in place of the session `id`, unless another took its place
/// meanwhile (the user picked other devices): the watchdog's way to restart the audio it judged.
/// `None` if another runs.
pub fn start_audio_instead_of(
    state: &AppState,
    id: u64,
    settings: &AudioSettings,
) -> AppResult<Option<AudioInfo>> {
    let _starting = lock(&state.audio_start);
    if lock(&state.session).as_ref().map(|s| s.id) != Some(id) {
        return Ok(None);
    }
    start(state, settings).map(Some)
}

/// Starts the saved audio again unless it is what runs (the inputs it was refused were allowed
/// meanwhile); whether it started.
pub fn restart_saved_audio(state: &AppState) -> AppResult<bool> {
    let _starting = lock(&state.audio_start);
    let saved = state.preferences.get().audio;
    let running = lock(&state.session).as_ref().map(|s| s.settings.clone());
    let saved = AudioSettings { simulate: running.as_ref().map_or(saved.simulate, |r| r.simulate), ..saved };
    if running.as_ref() == Some(&saved) {
        return Ok(false);
    }
    start(state, &saved).map(|_| true)
}

/// How long the clicks of a latency measurement play.
const LATENCY_MEASURE: Duration = Duration::from_secs(3);

/// The round trip from output `output` to input `input` (from 1, as printed on the card),
/// joined by a cable: the devices of the audio running, free of it for the measurement, which
/// then starts again where it was.
#[tauri::command]
pub async fn measure_latency(app: AppHandle, output: usize, input: usize) -> AppResult<LatencyDto> {
    blocking(move || {
        let state = app.state::<AppState>();
        let _starting = lock(&state.audio_start);
        let running = lock(&state.session).as_ref().map(|s| s.settings.clone());
        let settings = running.unwrap_or_else(|| state.preferences.read(|p| p.audio.clone()));
        let mut devices = audio_host::stream_settings(&settings);
        // a card or a driver both ways listens where it plays
        if one_device(settings.backend) {
            devices.input_device = devices.input_device.or_else(|| devices.output_device.clone());
        }
        let (_, measured) = restart(&state, &settings, || {
            latency::measure(&devices, output.saturating_sub(1), input.saturating_sub(1), LATENCY_MEASURE)
        })?;
        let measured = measured?;
        Ok(LatencyDto {
            round_trip_ms: measured.round_trip.as_secs_f64() * 1000.0,
            sample_rate: measured.format.sample_rate,
        })
    })
    .await
}

fn start(state: &AppState, settings: &AudioSettings) -> AppResult<AudioInfo> {
    restart(state, settings, || ()).map(|(info, ())| info)
}

/// Starts the audio with `settings`, the one running stopped; `between` runs in the meantime,
/// the devices free.
fn restart<T>(
    state: &AppState,
    settings: &AudioSettings,
    between: impl FnOnce() -> T,
) -> AppResult<(AudioInfo, T)> {
    let places = state.carry_places();
    // the device may not open twice; closed outside the lock, as it waits for the audio thread
    let previous = state.replace_session(None);
    let simulated = previous.as_ref().is_some_and(|s| s.info.simulated);
    drop(previous);
    let between = between();
    if settings.simulate {
        state.simulate_turntable(simulated);
    }
    let timecodes = lock(&state.decks).map(|d| d.timecode);
    let mut session = audio_host::start(settings, state.preferences.read(|p| p.dj.decks()), timecodes)?;
    state.share_turntable_records(&session.info);
    state.restore(&mut session, &places);
    let info = session.info.clone();
    // held from before the session is in place: a recording started on it meanwhile would
    // have its writer, on the live engine, waited for by the resume
    let mut recorder = lock(&state.recorder);
    state.replace_session(Some(session));
    // a recording goes on, in a file of its own
    if let Some(Err(e)) = recorder.as_mut().map(|r| r.resume(state)) {
        error!("recording ended, not resumed: {e}");
        if let Some(recorder) = recorder.take() {
            recorder.stop(state);
        }
    }
    Ok((info, between))
}
