//! The controllers and their profiles, and the actions they drive.

use crate::controller_profiles;
use crate::controllers::Shown;
use prisme_control::{Kind, Label, Mapping, Target};
use serde::Serialize;

/// An action a key or a controller can drive, as the interface lists it: for the keys, the
/// controllers' mappings and the command palette.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "Target"))]
pub struct TargetDto {
    pub id: String,
    pub kind: Kind,
    /// The deck it acts on: a deck's, or a load's.
    pub deck: Option<usize>,
    pub group: Label,
    pub label: Label,
    /// Of a deck in use, or of none.
    pub in_use: bool,
}

impl TargetDto {
    pub fn of(target: &Target, decks: usize) -> Self {
        Self {
            id: target.to_string(),
            kind: target.kind(),
            deck: target.deck(),
            group: target.group(),
            label: target.label(),
            in_use: target.in_use(decks),
        }
    }
}

/// The controllers as the interface shows them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "ControllersState"))]
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
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "Mapped"))]
pub struct MappedDto {
    pub target: TargetDto,
    pub devices: Vec<DeviceMappingsDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "DeviceMappings"))]
pub struct DeviceMappingsDto {
    pub device: String,
    pub mappings: Vec<Mapping>,
}

/// What drives what, by action and by controller in the order first met: the profiles of the
/// controllers in use, and what was learnt. An action this version does not know is left out.
fn mapped(mappings: &[Mapping], decks: usize) -> Vec<MappedDto> {
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

/// The profiles Prisme knows, as the settings list them.
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

/// A controller, MIDI or HID.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "Controller"))]
pub struct ControllerDto {
    /// Its MIDI port's name, or the name Prisme gives a HID device.
    pub name: String,
    pub enabled: bool,
    pub plugged: bool,
    /// The model Prisme knows it as, its controls mapped by its profile.
    pub profile: Option<String>,
    /// The file of its profile.
    pub profile_file: Option<String>,
    /// The profiles the DJ may choose for it: its model's, then the variants of it.
    pub offered: Vec<OfferedProfileDto>,
    /// Its profile was tried on the controller itself, not only read from another program's.
    pub verified: bool,
    /// Where its profile comes from.
    pub profile_source: Option<crate::controller_profiles::Source>,
    /// Prisme's model that the DJ's own profile takes the place of on it.
    pub replaces: Option<String>,
    /// The file its profile is exported as, by default.
    pub export_file: String,
    /// Listed with the controllers: a MIDI port, a HID device of a model Prisme knows, or one in
    /// use (most other HID devices are no controllers).
    pub listed: bool,
    /// What it drives can be written as a profile: it is in use and plugged in.
    pub can_export: bool,
}

/// The controllers plugged in and those in use that are not.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "Controllers"))]
pub struct ControllersDto {
    pub controllers: Vec<ControllerDto>,
    /// A controller is open: controls can be learnt.
    pub can_learn: bool,
}

/// A controller in use opened again: its device, and the model it is.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "Opened"))]
pub struct OpenedDto {
    pub device: String,
    pub model: String,
}

/// A profile the DJ may choose for a controller.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "OfferedProfile"))]
pub struct OfferedProfileDto {
    pub file: String,
    pub name: String,
}

/// The controller models Prisme knows, and where the DJ's own profiles go.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "ControllerProfiles"))]
pub struct ControllerProfilesDto {
    pub profiles: Vec<ControllerProfileDto>,
    /// The models known (their variants aside): all of them, those updated since, the DJ's own.
    pub counts: ProfileCountsDto,
    /// The DJ's own profiles that take the place of Prisme's of the same file name.
    pub replacing: Vec<OfferedProfileDto>,
    /// The DJ's files that could not be read, and why (`file: why`).
    pub unreadable: Vec<String>,
    /// The folder of the DJ's own profiles, if Prisme has a data folder.
    pub folder: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "ControllerProfile"))]
pub struct ControllerProfileDto {
    pub file: String,
    pub name: String,
    pub source: crate::controller_profiles::Source,
    pub verified: bool,
    /// Another profile of a model's (see `Profile::variant`): no model of its own.
    /// Another profile of a model's: counted with it.
    #[serde(skip)]
    pub variant: bool,
    /// Prisme's model whose profile of the same file name this one, the DJ's own, replaces.
    pub replaces: Option<String>,
}

/// How many models come from where, their variants aside.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "ProfileCounts"))]
pub struct ProfileCountsDto {
    pub all: usize,
    pub updated: usize,
    pub own: usize,
}

impl ProfileCountsDto {
    pub fn of(profiles: &[ControllerProfileDto]) -> Self {
        let models = || profiles.iter().filter(|p| !p.variant);
        let count = |source| models().filter(|p| p.source == source).count();
        use crate::controller_profiles::Source;
        Self { all: models().count(), updated: count(Source::Updated), own: count(Source::Own) }
    }
}

/// An action the interface performs that a controller drove: a key's value, or an encoder's
/// whole steps.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "UiAction"))]
pub struct UiActionDto {
    pub target: String,
    pub value: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller_profiles::Source;
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
