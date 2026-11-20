//! Controllers, MIDI and HID (see `controllers`): what the interface shows of them comes with
//! its section (`ControllersStateDto`), shown again after each command that changes it.

use crate::commands::targets::TargetDto;
use crate::controller_profiles;
use crate::controllers::{Controllers, Shown};
use crate::dto::{
    ControllerProfileDto, ControllerProfilesDto, ControllersDto, OfferedProfileDto, ProfileCountsDto,
};
use crate::error::{AppError, AppResult};
use crate::midi_clock::MidiClock;
use crate::state::AppState;
use prisme_control::{Encoding, Mapping};
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

/// The controllers as the interface shows them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllersStateDto {
    #[serde(flatten)]
    pub listed: ControllersDto,
    /// The controller models Prisme knows.
    pub profiles: ControllerProfilesDto,
    /// What drives what, by action and by controller.
    pub mapped: Vec<MappedDto>,
    /// Where the MIDI clock goes.
    pub clock_output: Option<String>,
    /// The action the next control moved will drive.
    pub learning: Option<String>,
}

impl ControllersStateDto {
    pub fn of(shown: Shown, decks: usize) -> Self {
        Self {
            mapped: mapped(&shown.mappings, decks),
            listed: shown.listed,
            profiles: profiles_dto(&shown.profiles.0),
            clock_output: shown.clock_output,
            learning: shown.learning,
        }
    }
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

/// Runs `change` on the controllers, then shows them.
fn changing<R>(app: &AppHandle, change: impl FnOnce(&Controllers) -> R) -> R {
    let controllers = app.state::<Controllers>();
    let changed = change(&controllers);
    controllers.show(&app.state::<AppState>());
    changed
}

/// Looks for the controllers plugged in now (they are looked for twice a second besides).
#[tauri::command]
pub fn look_for_controllers(state: State<'_, AppState>, controllers: State<'_, Controllers>) {
    controllers.look(&state);
}

#[tauri::command]
pub fn set_controller_enabled(app: AppHandle, name: String, enabled: bool) -> AppResult<()> {
    changing(&app, |c| c.set_enabled(&app, &name, enabled))
}

/// Drives the controller `name` by `file`, one of the profiles offered for it (a variant of its
/// model's), from now on.
#[tauri::command]
pub fn choose_controller_profile(app: AppHandle, name: String, file: String) -> AppResult<()> {
    changing(&app, |c| c.choose_profile(&app, &name, &file))
}

/// The models of the controllers in use that another program holds.
#[tauri::command]
pub fn waiting_controllers(controllers: State<'_, Controllers>) -> Vec<String> {
    controllers.waiting()
}

/// The DJ chose not to use the controller just plugged in: it is not offered again.
#[tauri::command]
pub fn decline_controller(app: AppHandle, name: String) {
    changing(&app, |c| c.decline(&name));
}

/// The next control moved on a controller in use will drive `target`, read as a key, a fader
/// or an encoder as its action takes.
#[tauri::command]
pub fn learn_control(app: AppHandle, target: String) {
    changing(&app, |c| c.learn(target));
}

/// How the encoder of `target` on `device` sends its steps (a jog that turns the wrong way
/// or by leaps is read with another encoding).
#[tauri::command]
pub fn set_control_encoding(app: AppHandle, device: String, target: String, encoding: Encoding) {
    changing(&app, |c| c.set_encoding(&device, &target, encoding));
}

#[tauri::command]
pub fn cancel_learning(app: AppHandle) {
    changing(&app, Controllers::cancel_learning);
}

/// Takes `target` off the controller `device`.
#[tauri::command]
pub fn forget_control(app: AppHandle, device: String, target: String) {
    changing(&app, |c| c.forget(&device, &target));
}

/// The MIDI outputs the clock can go to.
#[tauri::command]
pub fn midi_outputs() -> AppResult<Vec<String>> {
    MidiClock::outputs()
}

/// Sends the MIDI clock to `name` (`None`: nowhere); kept for the next start.
#[tauri::command]
pub fn set_midi_clock_output(app: AppHandle, name: Option<String>) -> AppResult<()> {
    app.state::<MidiClock>().set_output(name.as_deref())?;
    changing(&app, |c| c.remember_clock_output(name));
    Ok(())
}

/// Reads the profiles again: the DJ's own changed.
#[tauri::command]
pub fn reload_controller_profiles(app: AppHandle) {
    controller_profiles::reload();
    changing(&app, Controllers::profiles_changed);
}

/// Fetches the profiles published since: how many came.
#[tauri::command]
pub async fn update_controller_profiles(app: AppHandle) -> AppResult<usize> {
    let came = controller_profiles::update().await.map_err(AppError::Other)?;
    changing(&app, Controllers::profiles_changed);
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
