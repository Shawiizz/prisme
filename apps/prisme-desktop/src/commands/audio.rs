use crate::audio_host;
use crate::dto::{AudioInfo, AudioSettings, DeviceDto, TimecodeFormatDto};
use crate::error::AppResult;
use crate::state::{lock, AppState};
use prisme_audio_io::DeviceKind;
use tauri::State;

#[tauri::command]
pub fn list_devices() -> AppResult<Vec<DeviceDto>> {
    Ok(prisme_audio_io::list_devices()?
        .into_iter()
        .map(|d| DeviceDto {
            kind: match d.kind {
                DeviceKind::Input => "input",
                DeviceKind::Output => "output",
            },
            name: d.name,
            channels: d.channels,
            sample_rate: d.sample_rate,
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

/// (Re)starts the audio and restores the decks.
pub fn start_audio(state: &AppState, settings: &AudioSettings) -> AppResult<AudioInfo> {
    // the device may not open twice
    lock(&state.session).take();
    if settings.simulate {
        // the simulated needle starts 10 s into the record: start the track there
        let mut decks = lock(&state.decks);
        decks[0].vinyl_enabled = true;
        decks[0].lead_in_s = prisme_timecode::testing::SimulatedTurntable::START_S;
    }
    let mut session = audio_host::start(settings)?;
    state.restore(&mut session.controller);
    let info = session.info.clone();
    *lock(&state.session) = Some(session);
    Ok(info)
}
