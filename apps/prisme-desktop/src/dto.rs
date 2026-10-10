//! Data exchanged with the frontend (camelCase JSON).

use prisme_engine::{DeckStatus, EngineStatus, VinylMode};
use prisme_timecode::SignalState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDto {
    pub name: String,
    /// "input" or "output"
    pub kind: &'static str,
    pub channels: u16,
    pub sample_rate: u32,
    pub is_default: bool,
}

/// Audio settings chosen by the user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioSettings {
    pub output_device: Option<String>,
    /// Turntable input (DVS interface). None = no turntables.
    pub input_device: Option<String>,
    /// Feed deck 1 with a simulated turntable instead of the input.
    pub simulate: bool,
    pub timecode_format: String,
    pub buffer_frames: Option<u32>,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            output_device: None,
            input_device: None,
            simulate: false,
            timecode_format: prisme_timecode::SERATO_CV02_A.id.to_string(),
            buffer_frames: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioInfo {
    pub sample_rate: u32,
    pub output_channels: usize,
    pub input_channels: usize,
    pub simulated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimecodeFormatDto {
    pub id: &'static str,
    pub name: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaveformDto {
    pub slices_per_second: u32,
    pub peak: Vec<u8>,
    pub low: Vec<u8>,
    pub mid: Vec<u8>,
    pub high: Vec<u8>,
}

impl From<&prisme_analysis::Waveform> for WaveformDto {
    fn from(w: &prisme_analysis::Waveform) -> Self {
        Self {
            slices_per_second: w.slices_per_second,
            peak: w.peak.clone(),
            low: w.low.clone(),
            mid: w.mid.clone(),
            high: w.high.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TempoDto {
    pub bpm: f64,
    pub first_beat_s: f64,
}

impl From<&prisme_analysis::Tempo> for TempoDto {
    fn from(t: &prisme_analysis::Tempo) -> Self {
        Self { bpm: t.bpm, first_beat_s: t.first_beat_s }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyDto {
    /// "Am", "F#"...
    pub name: &'static str,
    /// "8A", "2B"...
    pub camelot: String,
}

impl From<&prisme_analysis::KeyEstimate> for KeyDto {
    fn from(k: &prisme_analysis::KeyEstimate) -> Self {
        Self { name: k.key.name(), camelot: k.key.camelot() }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackInfoDto {
    pub deck: usize,
    pub path: String,
    pub title: String,
    pub artist: Option<String>,
    pub duration_s: f64,
    pub sample_rate: u32,
    pub waveform: WaveformDto,
    pub tempo: Option<TempoDto>,
    pub key: Option<KeyDto>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum VinylModeDto {
    Absolute,
    Relative,
}

impl From<VinylModeDto> for VinylMode {
    fn from(m: VinylModeDto) -> Self {
        match m {
            VinylModeDto::Absolute => VinylMode::Absolute,
            VinylModeDto::Relative => VinylMode::Relative,
        }
    }
}

impl From<VinylMode> for VinylModeDto {
    fn from(m: VinylMode) -> Self {
        match m {
            VinylMode::Absolute => VinylModeDto::Absolute,
            VinylMode::Relative => VinylModeDto::Relative,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckSettingsDto {
    pub pitch: f64,
    pub volume: f32,
    pub vinyl_enabled: bool,
    pub vinyl_mode: VinylModeDto,
    pub lead_in_s: f64,
}

/// Settings the backend applies to the engine (the UI mirrors them at startup).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MixerStateDto {
    pub decks: Vec<DeckSettingsDto>,
    pub crossfader: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VinylDto {
    pub enabled: bool,
    pub mode: VinylModeDto,
    /// "none", "bounce" or "ok"
    pub signal: &'static str,
    pub record_speed: f64,
    pub record_position_s: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckStatusDto {
    pub loaded: bool,
    pub playing: bool,
    pub position_s: f64,
    pub duration_s: f64,
    pub speed: f64,
    pub vinyl: VinylDto,
}

impl From<&DeckStatus> for DeckStatusDto {
    fn from(d: &DeckStatus) -> Self {
        Self {
            loaded: d.loaded,
            playing: d.playing,
            position_s: d.position_s,
            duration_s: d.duration_s,
            speed: d.speed,
            vinyl: VinylDto {
                enabled: d.vinyl.enabled,
                mode: d.vinyl.mode.into(),
                signal: match d.vinyl.signal {
                    SignalState::None => "none",
                    SignalState::Bounce => "bounce",
                    SignalState::Ok => "ok",
                },
                record_speed: d.vinyl.record_speed,
                record_position_s: d.vinyl.record_position_s,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusDto {
    pub audio_running: bool,
    pub decks: Vec<DeckStatusDto>,
    pub main_peak: f32,
}

impl StatusDto {
    pub fn from_engine(s: &EngineStatus) -> Self {
        Self {
            audio_running: true,
            decks: s.decks[..s.deck_count].iter().map(DeckStatusDto::from).collect(),
            main_peak: s.main_peak,
        }
    }

    pub fn stopped() -> Self {
        Self { audio_running: false, decks: Vec::new(), main_peak: 0.0 }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntryDto {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirListingDto {
    pub path: String,
    pub parent: Option<String>,
    pub folders: Vec<DirEntryDto>,
    pub tracks: Vec<DirEntryDto>,
}
