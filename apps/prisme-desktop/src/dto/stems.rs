//! The separation into stems: its models and its queue.

use prisme_actions::stems::Separation as SeparationDto;
use serde::Serialize;

/// The separation queue.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StemsProgressDto {
    pub running: bool,
    /// Tracks left, the one being separated included.
    pub queued: usize,
    pub done: usize,
    pub failed: usize,
    /// The track being separated, and how far (0 to 1).
    pub current: Option<String>,
    pub fraction: f64,
    /// The GPU (and API) or "CPU" doing the work.
    pub device: Option<String>,
}

/// A separation model: installed, or how far its download is.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatusDto {
    /// `vocals` or `drums-bass`.
    pub id: &'static str,
    pub installed: bool,
    pub downloading: Option<f64>,
    pub error: Option<String>,
    pub bytes: u64,
}

/// The separation as the interface shows it (`stems_service::Shown`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StemsStateDto {
    pub progress: StemsProgressDto,
    /// The tracks the queue held: those done, failed, and left.
    pub total: usize,
    /// The queue's share done, the track being separated as far as it is, from 0 to 1.
    pub overall: f64,
    /// How far the models chosen and installed separate a track.
    pub reach: SeparationDto,
    /// A graphics chip worth separating on is there; `None` until it was looked for.
    pub gpu: Option<bool>,
    /// The separation runs on it: it is there and the DJ lets it.
    pub on_gpu: bool,
    /// Tracks are separated as they are loaded or prepared.
    pub on_load: bool,
    /// How far the downloads under way are, by their bytes, from 0 to 1.
    pub downloading: Option<f64>,
    /// The models the DJ chose, as the settings list them.
    pub models: Vec<ModelRowDto>,
    /// What the large models, and the small one, have left to download.
    pub quality: ChoiceDto,
    pub fast: ChoiceDto,
    /// The small models suit this device: a phone, or no graphics chip worth it.
    pub fast_suits: bool,
    /// The choice the models are offered with.
    pub suggest_fast: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRowDto {
    #[serde(flatten)]
    pub status: ModelStatusDto,
    /// It can be downloaded now: the drums and bass come with the vocals only.
    pub ready: bool,
}

/// A choice of models: what it has left to download, and why its download failed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceDto {
    /// `None` once it is installed.
    pub bytes_left: Option<u64>,
    pub error: Option<String>,
}

/// What became of tracks to separate.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SeparateDto {
    Queued,
    /// No model is installed: they are offered for them.
    NeedsModels,
}

/// How far a row's track is separated, as its column shows it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum RowStemsDto {
    /// Being separated, so far (0 to 1).
    Separating {
        fraction: f64,
    },
    /// Waiting in the queue.
    Queued,
    /// The models installed would add stems to it.
    Addable {
        separation: SeparationDto,
    },
    Separated {
        separation: SeparationDto,
    },
}
