use crate::audio_host;
use crate::commands::blocking;
use crate::dto::{
    AudioChangeDto, AudioDraftDto, AudioInfo, AudioSettings, BackendChoiceDto, BackendDto, BufferSizeDto,
    DeviceDto, LatencyChannelsDto, LatencyDto, MixModeDto, TimecodeMakerDto,
};
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState, Section};
use prisme_audio_io::{latency, usb_asks, AudioError, Backend, DeviceKind};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, State};
use tracing::{error, warn};

/// Backends this computer offers (ASIO only with an ASIO driver installed).
#[tauri::command]
pub fn audio_backends() -> Vec<BackendChoiceDto> {
    Backend::available()
        .into_iter()
        .map(BackendDto::from)
        .map(|backend| BackendChoiceDto { backend, one_device: backend.one_device() })
        .collect()
}

/// The devices of the driver listed last, which the settings window's drafts are read against:
/// listed again as the window lists them (it opens, the audio moves, another driver is
/// drafted), not at each change of a draft (a driver may take a moment to list).
#[derive(Default)]
pub struct ListedDevices(Mutex<Option<(BackendDto, Vec<DeviceDto>)>>);

impl ListedDevices {
    /// The devices of `backend`, from `list` unless they were listed last: none if they could
    /// not be, asked again next time.
    fn of(&self, backend: BackendDto, list: impl FnOnce() -> AppResult<Vec<DeviceDto>>) -> Vec<DeviceDto> {
        if let Some((_, devices)) = lock(&self.0).as_ref().filter(|(b, _)| *b == backend) {
            return devices.clone();
        }
        match list() {
            Ok(devices) => {
                *lock(&self.0) = Some((backend, devices.clone()));
                devices
            }
            Err(e) => {
                warn!("the devices of {backend:?} not listed: {e}");
                Vec::new()
            }
        }
    }
}

/// The devices of `backend`, listed now.
#[tauri::command]
pub fn list_devices(listed: State<'_, ListedDevices>, backend: BackendDto) -> AppResult<Vec<DeviceDto>> {
    let devices = devices_of(backend)?;
    *lock(&listed.0) = Some((backend, devices.clone()));
    Ok(devices)
}

fn devices_of(backend: BackendDto) -> AppResult<Vec<DeviceDto>> {
    let devices = prisme_audio_io::list_devices(backend.into())?
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
        .collect();
    Ok(devices)
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
pub fn timecode_formats() -> Vec<TimecodeMakerDto> {
    TimecodeMakerDto::of(prisme_timecode::FORMATS)
}

/// The audio settings drafted in the settings window, `change` made if any (see
/// `AudioSettings::changed`), against those kept. Off the window's thread: a driver's devices
/// may take a moment to list.
#[tauri::command(async)]
pub fn change_audio_draft(
    state: State<'_, AppState>,
    listed: State<'_, ListedDevices>,
    draft: AudioSettings,
    change: Option<AudioChangeDto>,
) -> AudioDraftDto {
    let listed_of = |backend: BackendDto| listed.of(backend, || devices_of(backend));
    let backend = draft.backend;
    let first_output = || listed_of(backend).into_iter().find(|d| d.kind == "output").map(|d| d.name);
    let draft = match change {
        Some(change) => draft.changed(change, first_output),
        None => draft,
    };
    let devices = listed_of(draft.backend);
    AudioDraftDto::of(draft, &state.preferences.read(|p| p.audio.clone()), &devices)
}

/// Restarts the audio with new settings, if they changed, and keeps them for the next start; then
/// uses the mixer `mix_mode`, if given. Off the commands' threads: a USB card may wait for the DJ
/// to let Prisme open it.
#[tauri::command]
pub async fn apply_audio_settings(
    app: AppHandle,
    settings: AudioSettings,
    mix_mode: Option<MixModeDto>,
) -> AppResult<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        apply(&state, settings)?;
        if let Some(mode) = mix_mode.filter(|&mode| state.preferences.read(|p| p.mixer.mode) != mode) {
            crate::actions::mixer::set_mix_mode(&state, mode)?;
        }
        Ok(())
    })
    .await
}

/// How long the settings wait for the DJ to let Prisme open the USB card chosen.
const ANSWER: Duration = Duration::from_secs(120);

/// Restarts the audio with `settings` if they changed or it does not run, and keeps them for
/// the next start. A USB card chosen is asked for again, even refused, and the DJ's answer
/// awaited.
fn apply(state: &AppState, settings: AudioSettings) -> AppResult<()> {
    let saved = state.preferences.read(|p| p.audio.clone());
    let running = lock(&state.session).is_some();
    if settings == saved && running {
        return Ok(());
    }
    // kept as they start: no other start comes between
    let _starting = lock(&state.audio_start);
    usb_asks::ask_again();
    state.moved(Section::Audio);
    let deadline = Instant::now() + ANSWER;
    loop {
        let answers = usb_asks::answers();
        match start(state, &settings) {
            Err(AppError::Audio(AudioError::UsbAsked(_))) if Instant::now() < deadline => {
                usb_asks::wait_for_answer(answers, deadline.saturating_duration_since(Instant::now()));
            }
            // devices missing since the start stay chosen, the defaults playing meanwhile
            Err(AppError::Audio(AudioError::DeviceNotFound(_))) if settings.same_devices(&saved) => {
                start(state, &settings.on_default_devices())?;
                break;
            }
            started => {
                started?;
                break;
            }
        }
    }
    state.update_preferences(|p| p.audio = settings);
    state.moved(Section::Audio);
    Ok(())
}

/// The turntables turned on for the driver of the settings kept (`saved`), as the settings
/// would turn them on: the audio restarts.
pub fn turn_turntables_on(state: &AppState, saved: &AudioSettings) -> AppResult<()> {
    let devices = devices_of(saved.backend)?;
    let settings = saved.with_turntables(&devices).ok_or(AppError::NoTurntableInput(saved.backend))?;
    apply(state, settings)
}

/// The channels of the audio's devices a latency measurement may join (two each at least). Off
/// the window's thread: a driver's devices may take a moment to list.
#[tauri::command(async)]
pub fn latency_channels(state: State<'_, AppState>) -> LatencyChannelsDto {
    let running = lock(&state.session).as_ref().map(|s| (s.settings.clone(), s.info.clone()));
    let settings =
        running.as_ref().map_or_else(|| state.preferences.read(|p| p.audio.clone()), |r| r.0.clone());
    let (outputs, inputs) = running.map_or((0, 0), |(_, info)| (info.output_channels, info.input_channels));
    let input = settings.input_device.clone().or_else(|| settings.output_device.clone());
    let devices = prisme_audio_io::list_devices(settings.backend.into()).unwrap_or_default();
    let listed = devices.iter().find(|d| d.kind == DeviceKind::Input && Some(&d.name) == input.as_ref());
    LatencyChannelsDto {
        outputs: outputs.max(2),
        inputs: inputs.max(listed.map_or(0, |d| usize::from(d.channels))).max(2),
    }
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
        if settings.backend.one_device() {
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
    let asked = usb_asks::asked();
    let started = audio_host::start(settings, state.decks_in_use(), timecodes);
    if usb_asks::asked() != asked {
        state.moved(Section::Audio);
    }
    let mut session = started?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drivers_devices_are_listed_once_until_another_is_drafted() {
        let listed = ListedDevices::default();
        let lists = std::cell::Cell::new(0);
        let of = |backend| {
            listed.of(backend, || {
                lists.set(lists.get() + 1);
                Ok(Vec::new())
            })
        };
        of(BackendDto::Wasapi);
        of(BackendDto::Wasapi);
        assert_eq!(lists.get(), 1, "a change of the draft on the same driver");
        of(BackendDto::Asio);
        of(BackendDto::Wasapi);
        assert_eq!(lists.get(), 3);
    }

    #[test]
    fn a_driver_whose_devices_could_not_be_listed_is_asked_again() {
        let listed = ListedDevices::default();
        let failed = listed.of(BackendDto::Asio, || Err(AppError::Other("busy".into())));
        assert!(failed.is_empty());
        let device = DeviceDto {
            name: "Card".into(),
            kind: "output",
            channels: 2,
            sample_rate: 48_000,
            buffer_sizes: Vec::new(),
            is_default: true,
        };
        assert_eq!(listed.of(BackendDto::Asio, || Ok(vec![device])).len(), 1);
    }
}
