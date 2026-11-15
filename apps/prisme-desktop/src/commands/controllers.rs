//! MIDI controllers (see `controllers`).

use crate::controller_profiles;
use crate::controllers::Controllers;
use crate::dto::{ControllerDto, ControllerProfileDto, ControllerProfilesDto, LightDto};
use crate::error::{AppError, AppResult};
use crate::midi_clock::MidiClock;
use prisme_control::{Encoding, Mapping};
use tauri::{AppHandle, State};

/// The controllers plugged in, and those in use that are not.
#[tauri::command]
pub fn list_controllers(controllers: State<'_, Controllers>) -> AppResult<Vec<ControllerDto>> {
    let plugged = Controllers::plugged()?;
    let enabled = controllers.setup().enabled;
    let missing = enabled.iter().filter(|name| !plugged.contains(name)).cloned().collect::<Vec<_>>();
    let dto = |name: String, plugged: bool| {
        let profile = controller_profiles::of(&name);
        ControllerDto {
            enabled: enabled.contains(&name),
            profile: profile.as_ref().map(|p| p.name.clone()),
            verified: profile.is_some_and(|p| p.verified),
            profile_source: controller_profiles::source_of(&name),
            name,
            plugged,
        }
    };
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

/// What drives what: the profiles of the controllers in use, and what was learnt.
#[tauri::command]
pub fn controller_mappings(controllers: State<'_, Controllers>) -> Vec<Mapping> {
    controllers.mappings().all().to_vec()
}

/// The actions whose state the profiles of the controllers in use show on lights of their
/// own, beside the controls that drive them.
#[tauri::command]
pub fn controller_light_targets(controllers: State<'_, Controllers>) -> Vec<String> {
    controllers.light_targets()
}

/// The next control moved on a controller in use will drive `target`
/// (`controller-learned` says which).
#[tauri::command]
pub fn learn_control(controllers: State<'_, Controllers>, target: String, encoder: bool) {
    controllers.learn(target, encoder);
}

/// How the encoder of `target` on `device` sends its steps (a jog that turns the wrong way
/// or by leaps is read with another encoding).
#[tauri::command]
pub fn set_control_encoding(
    controllers: State<'_, Controllers>,
    device: String,
    target: String,
    encoding: Encoding,
) {
    controllers.set_encoding(&device, &target, encoding);
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

/// The controller models Prisme knows.
#[tauri::command]
pub fn controller_profiles() -> ControllerProfilesDto {
    profiles_dto(&controller_profiles::current())
}

/// Reads the profiles again: the DJ's own changed.
#[tauri::command]
pub fn reload_controller_profiles(controllers: State<'_, Controllers>) -> ControllerProfilesDto {
    let library = controller_profiles::reload();
    controllers.profiles_changed();
    profiles_dto(&library)
}

/// Fetches the profiles published since: how many came.
#[tauri::command]
pub async fn update_controller_profiles(controllers: State<'_, Controllers>) -> AppResult<usize> {
    let came = controller_profiles::update().await.map_err(AppError::Other)?;
    controllers.profiles_changed();
    Ok(came)
}

/// Writes what `device` drives now as a profile of the DJ's own: where it went.
#[tauri::command]
pub fn export_controller_profile(controllers: State<'_, Controllers>, device: String) -> AppResult<String> {
    let path = controller_profiles::save_own(&controllers.as_profile(&device)).map_err(AppError::Other)?;
    controllers.profiles_changed();
    Ok(path.display().to_string())
}

/// Shows the folder of the DJ's own profiles in the system's file manager.
#[tauri::command]
pub fn open_controller_profiles_folder() -> AppResult<()> {
    let folder = controller_profiles::own_folder().ok_or_else(|| AppError::Other("no data folder".into()))?;
    std::fs::create_dir_all(&folder)?;
    reveal(&folder)
}

#[cfg(desktop)]
fn reveal(folder: &std::path::Path) -> AppResult<()> {
    #[cfg(target_os = "windows")]
    let manager = "explorer";
    #[cfg(target_os = "macos")]
    let manager = "open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let manager = "xdg-open";
    std::process::Command::new(manager).arg(folder).spawn()?;
    Ok(())
}

#[cfg(mobile)]
fn reveal(_folder: &std::path::Path) -> AppResult<()> {
    Err(AppError::Other("a phone shows no folders".into()))
}

fn profiles_dto(library: &controller_profiles::Library) -> ControllerProfilesDto {
    ControllerProfilesDto {
        profiles: library
            .known
            .iter()
            .map(|k| ControllerProfileDto {
                file: k.file.clone(),
                name: k.profile.name.clone(),
                source: k.source,
                verified: k.profile.verified,
            })
            .collect(),
        unreadable: library.unreadable.clone(),
        folder: controller_profiles::own_folder().map(|f| f.display().to_string()),
    }
}
