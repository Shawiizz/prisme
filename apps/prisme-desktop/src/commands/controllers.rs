//! MIDI controllers (see `controllers`).

use crate::controllers::Controllers;
use crate::dto::{ControllerDto, LightDto};
use crate::error::AppResult;
use crate::midi_clock::MidiClock;
use prisme_control::{Encoding, Mapping};
use tauri::{AppHandle, State};

/// The controllers plugged in, and those in use that are not.
#[tauri::command]
pub fn list_controllers(controllers: State<'_, Controllers>) -> AppResult<Vec<ControllerDto>> {
    let plugged = Controllers::plugged()?;
    let enabled = controllers.setup().enabled;
    let missing = enabled.iter().filter(|name| !plugged.contains(name)).cloned().collect::<Vec<_>>();
    let dto = |name: String, plugged: bool| ControllerDto { enabled: enabled.contains(&name), name, plugged };
    Ok(plugged.into_iter().map(|n| dto(n, true)).chain(missing.into_iter().map(|n| dto(n, false))).collect())
}

#[tauri::command]
pub fn set_controller_enabled(
    app: AppHandle,
    controllers: State<'_, Controllers>,
    name: String,
    enabled: bool,
) -> AppResult<()> {
    controllers.set_enabled(&app, &name, enabled)
}

#[tauri::command]
pub fn controller_mappings(controllers: State<'_, Controllers>) -> Vec<Mapping> {
    controllers.setup().mappings.all().to_vec()
}

/// The next control moved on a controller in use will drive `target`
/// (`controller-learned` says which).
#[tauri::command]
pub fn learn_control(controllers: State<'_, Controllers>, target: String, encoder: bool) {
    let encoding = if encoder { Encoding::Relative } else { Encoding::Absolute };
    controllers.learn(target, encoding);
}

#[tauri::command]
pub fn cancel_learning(controllers: State<'_, Controllers>) {
    controllers.cancel_learning();
}

#[tauri::command]
pub fn forget_control(controllers: State<'_, Controllers>, target: String) {
    controllers.forget(&target);
}

#[tauri::command]
pub fn set_lights(controllers: State<'_, Controllers>, lights: Vec<LightDto>) {
    controllers.set_lights(&lights);
}

/// The MIDI outputs the clock can go to.
#[tauri::command]
pub fn midi_outputs() -> AppResult<Vec<String>> {
    MidiClock::outputs()
}

#[tauri::command]
pub fn midi_clock_output(clock: State<'_, MidiClock>) -> Option<String> {
    clock.output_name()
}

/// Sends the MIDI clock to `name` (`None`: nowhere); kept for the next start.
#[tauri::command]
pub fn set_midi_clock_output(
    clock: State<'_, MidiClock>,
    controllers: State<'_, Controllers>,
    name: Option<String>,
) -> AppResult<()> {
    clock.set_output(name.as_deref())?;
    controllers.remember_clock_output(name);
    Ok(())
}
