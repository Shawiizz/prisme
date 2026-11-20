//! The controllers and their profiles.

use serde::Serialize;

/// A controller, MIDI or HID.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
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
pub struct ControllersDto {
    pub controllers: Vec<ControllerDto>,
    /// A controller is open: controls can be learnt.
    pub can_learn: bool,
}

/// A controller in use opened again: its device, and the model it is.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedDto {
    pub device: String,
    pub model: String,
}

/// A profile the DJ may choose for a controller.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct OfferedProfileDto {
    pub file: String,
    pub name: String,
}

/// The controller models Prisme knows, and where the DJ's own profiles go.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
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
pub struct UiActionDto {
    pub target: String,
    pub value: f32,
}
