//! Keeps the audio going, with the decks where they were: restarts it when its device
//! comes back after a loss, when the engine failed, or when the driver stopped calling it,
//! moves it to the saved devices once they appear (the audio started on the default ones
//! because the interface was off), follows Windows' default output when it plays on it,
//! starts it when devices appear if there was none, and on the saved devices once the DJ lets
//! Prisme listen to their inputs or open their USB card. A sound card plugged in meanwhile is
//! offered to the DJ.

use crate::audio_host::{devices_available, stream_settings};
use crate::commands::audio::{restart_saved_audio, start_audio_instead_of, start_audio_unless_running};
use crate::dto::AudioSettings;
use crate::dto::BackendDto;
use crate::dto::CardOffer;
use crate::input_access;
use crate::state::{lock, AppState};
use prisme_audio_io::{
    base_device_name, default_output_name, list_devices, output_opens, system_devices, usb_asks, Backend,
    DeviceKind,
};
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tracing::{error, info, warn};

const CHECK: Duration = Duration::from_millis(500);
const RETRY: Duration = Duration::from_secs(2);
/// A lost device is tried again at once, then this often for a few times (a Bluetooth
/// headset or an interface back within a second), then every `RETRY`.
const QUICK_RETRY: Duration = Duration::from_millis(200);
const QUICK_RETRIES: usize = 5;
/// More engine failures than this within the window are not restarted: the failure
/// would only come back.
const MAX_RESTARTS: usize = 3;
const RESTART_WINDOW: Duration = Duration::from_secs(60);
/// Looks for the saved devices after the system's devices changed.
const LOOKS_AFTER_A_CHANGE: u32 = 5;
/// Sent with a [`CardOffer`] when a sound card is plugged in.
pub const CARD_PLUGGED_EVENT: &str = "card-plugged";

pub fn start(app: AppHandle) {
    std::thread::Builder::new()
        .name("prisme-audio-watchdog".into())
        .spawn(move || {
            let state = app.state::<AppState>();
            let mut restarts = Restarts::default();
            let mut last_periods = None;
            let mut last_look = Instant::now();
            let mut devices: Option<Vec<String>> = None;
            let mut looks_left = 0;
            // no audio since the last check: tried at once, then when devices change
            let mut silent = false;
            let mut default_output = default_output_name();
            // the system's devices at the last look, while the audio runs
            let mut plugged: Option<Vec<String>> = None;
            let mut answers = usb_asks::answers();
            loop {
                let seen = answers;
                answers = usb_asks::wait_for_answer(seen, CHECK);
                // a USB card let open (or refused): the saved devices are looked for again now
                let answered = answers != seen;
                if answered {
                    devices = None;
                }
                if input_access::newly_allowed() {
                    take_the_inputs(&state);
                    last_periods = None;
                }
                let session = lock(&state.session).as_ref().map(|s| {
                    let device = s.info.device.clone();
                    (s.id, s.device_lost(), s.engine_failed(), s.periods(), s.settings.clone(), device)
                });
                let Some((id, lost, failed, periods, settings, device)) = session else {
                    last_periods = None;
                    if !silent {
                        silent = true;
                        devices = None;
                    }
                    if answered || last_look.elapsed() >= RETRY {
                        last_look = Instant::now();
                        start_when_devices_change(&state, &mut devices);
                    }
                    continue;
                };
                silent = false;
                if lost {
                    info!("audio device lost, waiting for it to come back");
                    recover(&state, &settings);
                    last_periods = None;
                    continue;
                }
                if answered || last_look.elapsed() >= RETRY {
                    last_look = Instant::now();
                    let saved =
                        AudioSettings { simulate: settings.simulate, ..state.preferences.get().audio };
                    offer_cards_plugged(&state, &app, &settings, &saved, &device, &mut plugged);
                    if take_the_card_let_go(&state, id, &saved, &settings) {
                        last_periods = None;
                        continue;
                    }
                    if saved == settings {
                        devices = None;
                    } else if move_to_saved(&state, id, &saved, &settings, &mut devices, &mut looks_left) {
                        last_periods = None;
                        continue;
                    }
                    if follow_default_output(&state, id, &settings, &device, &mut default_output) {
                        last_periods = None;
                        continue;
                    }
                }
                let stalled = last_periods == Some(periods);
                last_periods = Some(periods);
                // a phone's system stops calling an application in the background: its audio
                // starts again when it comes back, unless a deck plays meanwhile
                let paused = stalled
                    && !failed
                    && state.hidden.load(std::sync::atomic::Ordering::Relaxed)
                    && !state.is_playing();
                if (failed || stalled) && !paused && restarts.allow(Instant::now()) {
                    warn!("audio {}, restarting it", if failed { "engine failed" } else { "stalled" });
                    if let Err(e) = start_audio_instead_of(&state, id, &settings) {
                        error!("audio could not restart: {e}");
                    }
                    last_periods = None;
                }
            }
        })
        .expect("cannot start the audio watchdog");
}

/// `MainActivity.shown`: Prisme came into sight or went out of it. Android may stop calling the
/// audio of an application out of sight: the watchdog waits for it then, unless a deck plays.
/// Told by the activity, which Android calls on leaving the screen, unlike the page it freezes.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_com_shawiizz_prisme_MainActivity_shown(
    _env: jni::JNIEnv,
    _activity: jni::objects::JObject,
    shown: jni::sys::jboolean,
) {
    if let Some(app) = crate::android_jni::app() {
        app.state::<AppState>()
            .hidden
            .store(shown == jni::sys::JNI_FALSE, std::sync::atomic::Ordering::Relaxed);
    }
}

/// The card chosen, that another application held as the audio started, taken as soon as it
/// lets go of it (tried without stopping the audio meanwhile). True once the audio of session
/// `id` moved to it.
fn take_the_card_let_go(state: &AppState, id: u64, saved: &AudioSettings, current: &AudioSettings) -> bool {
    let held = lock(&state.busy_output).is_some() && saved.output_device.is_some();
    if !held || saved.same_devices(current) || !output_opens(&stream_settings(saved)) {
        return false;
    }
    info!("the sound card held by another application is free, moving to it");
    match start_audio_instead_of(state, id, saved) {
        Ok(_) => {
            *lock(&state.busy_output) = None;
            state.moved(crate::state::Section::Audio);
        }
        Err(e) => warn!("the sound card let go does not open yet: {e}"),
    }
    true
}

/// While the audio runs on other devices than the saved ones, looks for these whenever
/// the system's devices change (loading the drivers to look is slow, and they must not be
/// loaded over and over), a few times as an interface takes a moment to come up. True
/// once the audio of session `id` has moved, or other audio took its place.
fn move_to_saved(
    state: &AppState,
    id: u64,
    saved: &AudioSettings,
    current: &AudioSettings,
    devices: &mut Option<Vec<String>>,
    looks_left: &mut u32,
) -> bool {
    let now = system_devices();
    if devices.as_ref() != Some(&now) {
        *devices = Some(now);
        *looks_left = LOOKS_AFTER_A_CHANGE;
    }
    if *looks_left == 0 {
        return false;
    }
    *looks_left -= 1;
    if !devices_available(saved) {
        return false;
    }
    *looks_left = 0;
    info!("saved audio devices found, moving to them");
    match start_audio_instead_of(state, id, saved) {
        Ok(_) => {}
        Err(e) => {
            warn!("saved audio devices unusable ({e}), back to the previous ones");
            if let Err(e) = start_audio_unless_running(state, current) {
                error!("audio could not restart: {e}");
            }
        }
    }
    true
}

/// Audio on the system's default output (no device chosen) moves when the default changes from
/// `last` to another device than `device`: a headset connected takes the sound, as in every
/// other program. True once the audio of session `id` moved, or other audio took its place.
fn follow_default_output(
    state: &AppState,
    id: u64,
    settings: &AudioSettings,
    device: &str,
    last: &mut Option<String>,
) -> bool {
    if settings.output_device.is_some() || settings.backend != BackendDto::default() {
        return false;
    }
    let now = default_output_name();
    if now == *last {
        return false;
    }
    *last = now.clone();
    match now {
        Some(default) if default != device => {
            info!("the system plays on {default} now, the audio follows");
            if let Err(e) = start_audio_instead_of(state, id, settings) {
                error!("audio could not follow the default output: {e}");
            }
            true
        }
        _ => false,
    }
}

/// Offers the DJ a sound card plugged in since the last look (`plugged`), unless the audio
/// plays on it or on the system's default output (which a card plugged in may take), the saved
/// devices are back (the audio moves to them), or the DJ chose not to use it. Using it plays on
/// it, and listens to it for the turntables if they are listened to.
fn offer_cards_plugged(
    state: &AppState,
    app: &AppHandle,
    settings: &AudioSettings,
    saved: &AudioSettings,
    device: &str,
    plugged: &mut Option<Vec<String>>,
) {
    let now = system_devices();
    let Some(before) = plugged.replace(now.clone()) else { return };
    if !saved.same_devices(settings) && devices_available(saved) {
        return;
    }
    let declined = state.preferences.read(|p| p.declined_cards.clone());
    let fresh: Vec<&String> = now
        .iter()
        .filter(|name| !before.contains(name) && *name != device)
        .filter(|name| !declined.contains(&base_device_name(card_name(name))))
        .collect();
    if fresh.is_empty() {
        return;
    }
    let devices = list_devices(Backend::SYSTEM).unwrap_or_default();
    let of_kind = |kind: DeviceKind| {
        devices.iter().find(|d| d.kind == kind && fresh.contains(&&d.name)).map(|d| d.name.clone())
    };
    let (output, input) = (of_kind(DeviceKind::Output), of_kind(DeviceKind::Input));
    let Some(name) = output.as_deref().or(input.as_deref()).map(|n| card_name(n).to_owned()) else { return };
    // on the system's default output, only a controller's card is offered: any other plugged
    // in may become the default, which the audio follows
    let controller = crate::controller_profiles::current().all_in_one_of_card(&name);
    if settings.output_device.is_none() && controller.is_none() {
        return;
    }
    let offer = CardOffer {
        controller,
        name,
        settings: AudioSettings {
            backend: Backend::SYSTEM.into(),
            output_device: output.clone().or_else(|| settings.output_device.clone()),
            input_device: if settings.input_device.is_some() {
                input.or_else(|| settings.input_device.clone())
            } else {
                None
            },
            buffer_frames: None,
            ..settings.clone()
        },
    };
    info!("sound card {} plugged in, offered", offer.name);
    let _ = app.emit(CARD_PLUGGED_EVENT, offer);
}

/// The card of a device of the system, as its endpoint names it: `Speakers (UMC404HD 192k)`
/// is of `UMC404HD 192k`; a name without one is the card's.
pub fn card_name(endpoint: &str) -> &str {
    let Some(inner) = endpoint.strip_suffix(')') else { return endpoint };
    let mut depth = 0;
    for (at, c) in inner.char_indices().rev() {
        match c {
            ')' => depth += 1,
            '(' if depth == 0 => return inner[at + 1..].trim(),
            '(' => depth -= 1,
            _ => {}
        }
    }
    endpoint
}

/// With no audio at all (no device when Prisme started, or a restart that failed), starts it
/// on the saved devices, else the default ones, whenever the system's devices change.
fn start_when_devices_change(state: &AppState, devices: &mut Option<Vec<String>>) {
    let now = system_devices();
    if devices.as_ref() == Some(&now) {
        return;
    }
    *devices = Some(now);
    let saved = state.preferences.get().audio;
    let defaults = AudioSettings { simulate: saved.simulate, ..Default::default() };
    let started =
        start_audio_unless_running(state, &saved).or_else(|_| start_audio_unless_running(state, &defaults));
    if let Ok(Some(_)) = started {
        info!("audio started on the devices there are");
    }
}

/// The DJ let Prisme listen to the inputs: the saved audio, which was refused them, starts
/// unless it runs.
fn take_the_inputs(state: &AppState) {
    match restart_saved_audio(state) {
        Ok(false) => {}
        Ok(true) => {
            info!("the inputs allowed, the saved audio starts");
        }
        Err(e) => warn!("the inputs allowed, but the saved audio could not start: {e}"),
    }
}

/// Tries `settings` again until their device is back, unless other settings start the audio
/// meanwhile (the user picked other devices): at once when the DJ answers for a USB card, which
/// Android asks for again once plugged in again.
fn recover(state: &AppState, settings: &AudioSettings) {
    let mut waits = std::iter::repeat_n(QUICK_RETRY, QUICK_RETRIES).chain(std::iter::repeat(RETRY));
    // each reason told once: the device may stay away for hours
    let mut told = String::new();
    loop {
        let answers = usb_asks::answers();
        match start_audio_unless_running(state, settings) {
            Ok(Some(_)) => {
                info!("audio device back");
                return;
            }
            // other settings run it: the user picked other devices
            Ok(None) => return,
            Err(e) => {
                let reason = e.to_string();
                if reason != told {
                    info!("audio unavailable, tried again until it is back: {reason}");
                    told = reason;
                }
            }
        }
        usb_asks::wait_for_answer(answers, waits.next().unwrap_or(RETRY));
    }
}

/// Recent restarts, to stop restarting a failure that keeps coming back.
#[derive(Default)]
struct Restarts(VecDeque<Instant>);

impl Restarts {
    fn allow(&mut self, now: Instant) -> bool {
        while self.0.front().is_some_and(|&t| now.duration_since(t) > RESTART_WINDOW) {
            self.0.pop_front();
        }
        let allowed = self.0.len() < MAX_RESTARTS;
        if allowed {
            self.0.push_back(now);
        }
        allowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_of_an_endpoint() {
        assert_eq!(card_name("Speakers (UMC404HD 192k)"), "UMC404HD 192k");
        assert_eq!(card_name("Haut-parleurs (Realtek(R) Audio)"), "Realtek(R) Audio");
        assert_eq!(card_name("Line (2- SL3)"), "2- SL3");
        assert_eq!(card_name("Built-in Output"), "Built-in Output");
    }

    #[test]
    fn restarts_are_limited_within_the_window() {
        let mut restarts = Restarts::default();
        let start = Instant::now();
        assert!((0..MAX_RESTARTS).all(|i| restarts.allow(start + Duration::from_secs(i as u64))));
        assert!(!restarts.allow(start + Duration::from_secs(30)));
        assert!(restarts.allow(start + RESTART_WINDOW + Duration::from_secs(1)));
    }
}
