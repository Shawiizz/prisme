//! Controllers, MIDI and HID (see `controllers`).

use crate::commands::targets::{self, TargetDto};
use crate::controller_profiles;
use crate::controllers::Controllers;
use crate::dto::{
    ControllerDto, ControllerProfileDto, ControllerProfilesDto, ControllersDto, LightDto, OfferedProfileDto,
    ProfileCountsDto,
};
use crate::error::{AppError, AppResult};
use crate::midi_clock::MidiClock;
use crate::state::AppState;
use prisme_control::hid;
use prisme_control::{Encoding, Mapping};
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, State};

/// The controllers plugged in, and those in use that are not.
#[tauri::command]
pub fn list_controllers(controllers: State<'_, Controllers>) -> ControllersDto {
    let plugged = Controllers::plugged();
    let setup = controllers.setup();
    let missing = setup.enabled.iter().filter(|name| !plugged.contains(name)).cloned().collect::<Vec<_>>();
    let library = controller_profiles::current();
    let dto = |name: String, plugged: bool| {
        let known = library.chosen(&name, setup.profiles.get(&name).map(String::as_str));
        let models = known.map(|k| &k.file) == library.of(&name).map(|k| &k.file);
        let (enabled, hid) = (setup.enabled.contains(&name), hid::ids_of(&name).is_some());
        ControllerDto {
            listed: !hid || known.is_some() || enabled,
            can_export: enabled && plugged,
            enabled,
            profile: known.map(|k| k.profile.name.clone()),
            profile_file: known.map(|k| k.file.clone()),
            offered: library
                .offered(&name)
                .into_iter()
                .map(|k| OfferedProfileDto { file: k.file.clone(), name: k.profile.name.clone() })
                .collect(),
            verified: known.is_some_and(|k| k.profile.verified),
            profile_source: known.map(|k| k.source),
            replaces: library.replaced_on(&name).filter(|_| models).map(|k| k.profile.name.clone()),
            export_file: controller_profiles::file_name(&controllers.model_name(&name)),
            name,
            plugged,
        }
    };
    let controllers: Vec<ControllerDto> =
        plugged.into_iter().map(|n| dto(n, true)).chain(missing.into_iter().map(|n| dto(n, false))).collect();
    ControllersDto { can_learn: controllers.iter().any(|c| c.can_export), controllers }
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

/// Drives the controller `name` by `file`, one of the profiles offered for it (a variant of its
/// model's), from now on.
#[tauri::command]
pub fn choose_controller_profile(
    app: AppHandle,
    controllers: State<'_, Controllers>,
    name: String,
    file: String,
) -> AppResult<()> {
    controllers.choose_profile(&app, &name, &file)
}

/// The models of the controllers in use that another program holds.
#[tauri::command]
pub fn waiting_controllers(controllers: State<'_, Controllers>) -> Vec<String> {
    controllers.waiting()
}

/// The DJ chose not to use the controller just plugged in: it is not offered again.
#[tauri::command]
pub fn decline_controller(controllers: State<'_, Controllers>, name: String) {
    controllers.decline(&name);
}

/// An action the controllers in use drive, and what drives it on each.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MappedDto {
    pub target: TargetDto,
    pub devices: Vec<DeviceMappingsDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeviceMappingsDto {
    pub device: String,
    pub mappings: Vec<Mapping>,
}

/// What drives what, by action and by controller in the order first met: the profiles of the
/// controllers in use, and what was learnt. An action this version does not know is left out.
pub fn mapped(mappings: &[Mapping], decks: usize) -> Vec<MappedDto> {
    let mut mapped: Vec<MappedDto> = Vec::new();
    for mapping in mappings {
        let Some(target) = mapping.target.target() else { continue };
        let index = match mapped.iter().position(|m| m.target.id == mapping.target.as_str()) {
            Some(index) => index,
            None => {
                mapped.push(MappedDto { target: TargetDto::of(target, decks), devices: Vec::new() });
                mapped.len() - 1
            }
        };
        let devices = &mut mapped[index].devices;
        match devices.iter_mut().find(|d| d.device == mapping.device) {
            Some(device) => device.mappings.push(mapping.clone()),
            None => devices
                .push(DeviceMappingsDto { device: mapping.device.clone(), mappings: vec![mapping.clone()] }),
        }
    }
    mapped
}

#[tauri::command]
pub fn controller_mappings(
    state: State<'_, AppState>,
    controllers: State<'_, Controllers>,
) -> Vec<MappedDto> {
    mapped(controllers.mappings().all(), targets::decks(&state))
}

/// The actions whose light the interface tells (`set_lights`), of those the controllers in use
/// drive or show.
#[tauri::command]
pub fn controller_light_targets(controllers: State<'_, Controllers>) -> Vec<String> {
    controllers.told_targets()
}

/// The next control moved on a controller in use will drive `target`, read as a key, a fader
/// or an encoder as its action takes (`controller-learned` says which).
#[tauri::command]
pub fn learn_control(controllers: State<'_, Controllers>, target: String) {
    controllers.learn(target);
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

/// Takes `target` off the controller `device`.
#[tauri::command]
pub fn forget_control(controllers: State<'_, Controllers>, device: String, target: String) {
    controllers.forget(&device, &target);
}

#[tauri::command]
pub fn set_lights(controllers: State<'_, Controllers>, lights: Vec<LightDto>) {
    controllers.set_lights(lights);
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

/// Writes what `device` drives now as a profile, to `path`, where the DJ chose: what it drives
/// does not change.
#[tauri::command]
pub fn export_controller_profile(
    controllers: State<'_, Controllers>,
    device: String,
    path: PathBuf,
) -> AppResult<()> {
    controller_profiles::export(&controllers.as_profile(&device), &path).map_err(AppError::Other)
}

/// Shows the folder of the DJ's own profiles in the system's file manager.
#[tauri::command]
pub fn open_controller_profiles_folder() -> AppResult<()> {
    let folder = controller_profiles::own_folder().ok_or_else(|| AppError::Other("no data folder".into()))?;
    std::fs::create_dir_all(&folder)?;
    Ok(crate::folders::show(&folder)?)
}

fn profiles_dto(library: &controller_profiles::Library) -> ControllerProfilesDto {
    let profiles: Vec<ControllerProfileDto> = library
        .known
        .iter()
        .map(|k| ControllerProfileDto {
            file: k.file.clone(),
            name: k.profile.name.clone(),
            source: k.source,
            verified: k.profile.verified,
            variant: k.profile.variant,
            replaces: library.replaced.iter().find(|r| r.file == k.file).map(|r| r.profile.name.clone()),
        })
        .collect();
    let replacing = profiles
        .iter()
        .filter_map(|p| Some(OfferedProfileDto { file: p.file.clone(), name: p.replaces.clone()? }))
        .collect();
    ControllerProfilesDto {
        counts: ProfileCountsDto::of(&profiles),
        replacing,
        profiles,
        unreadable: library.unreadable.clone(),
        folder: controller_profiles::own_folder().map(|f| f.display().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::mapped;
    use crate::controller_profiles::Source;
    use crate::dto::{ControllerProfileDto, ProfileCountsDto};
    use prisme_control::{Control, Mapping, ProfileControl};

    #[test]
    fn the_mappings_are_told_by_action_then_by_controller_measured_ones_too() {
        let mapping = |device: &str, number, target: &str| {
            Mapping::new(
                device,
                ProfileControl {
                    control: Control::note(0, number),
                    target: target.into(),
                    ..Default::default()
                },
            )
        };
        let mappings = [
            mapping("DDJ", 1, "deck1.loop.2"),
            mapping("DDJ", 2, "deck1.play"),
            mapping("Xone", 3, "deck1.loop.2"),
            mapping("DDJ", 4, "deck1.loop.2"),
            mapping("DDJ", 5, "deck1.unknown"),
        ];
        let mapped = mapped(&mappings, 2);
        let told: Vec<(&str, Vec<(&str, usize)>)> = mapped
            .iter()
            .map(|m| {
                (
                    m.target.id.as_str(),
                    m.devices.iter().map(|d| (d.device.as_str(), d.mappings.len())).collect(),
                )
            })
            .collect();
        assert_eq!(told, [("deck1.loop.2", vec![("DDJ", 2), ("Xone", 1)]), ("deck1.play", vec![("DDJ", 1)])]);
        assert_eq!(mapped[0].target.label.english(), "Loop 2 beats");
    }

    #[test]
    fn the_models_are_counted_by_where_they_come_from_their_variants_aside() {
        let profile = |source, variant| ControllerProfileDto {
            file: String::new(),
            name: String::new(),
            source,
            verified: false,
            variant,
            replaces: None,
        };
        let profiles = [
            profile(Source::Shipped, false),
            profile(Source::Shipped, true),
            profile(Source::Updated, false),
            profile(Source::Own, false),
            profile(Source::Own, true),
        ];
        assert_eq!(ProfileCountsDto::of(&profiles), ProfileCountsDto { all: 3, updated: 1, own: 1 });
    }
}
