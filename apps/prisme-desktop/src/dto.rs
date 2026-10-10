//! Data exchanged with the frontend (camelCase JSON).

use prisme_audio_io::{Backend, StreamStats};
use prisme_engine::{DeckStatus, EngineStatus, LoopRegion, MixMode, VinylMode};
use prisme_library::{Cue, CueSlot};
use prisme_timecode::SignalState;
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDto {
    pub name: String,
    /// "input" or "output"
    pub kind: &'static str,
    pub channels: u16,
    pub sample_rate: u32,
    /// Periods the driver accepts (frames): `[min, max]`.
    pub buffer_range: Option<(u32, u32)>,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackendDto {
    #[default]
    Wasapi,
    Asio,
}

impl From<BackendDto> for Backend {
    fn from(b: BackendDto) -> Self {
        match b {
            BackendDto::Wasapi => Backend::Wasapi,
            BackendDto::Asio => Backend::Asio,
        }
    }
}

impl From<Backend> for BackendDto {
    fn from(b: Backend) -> Self {
        match b {
            Backend::Wasapi => BackendDto::Wasapi,
            Backend::Asio => BackendDto::Asio,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MixModeDto {
    /// Each deck on its own outputs, to a hardware mixer.
    #[default]
    External,
    /// The software mixer: master and headphones.
    Internal,
}

impl From<MixMode> for MixModeDto {
    fn from(m: MixMode) -> Self {
        match m {
            MixMode::External => MixModeDto::External,
            MixMode::Internal => MixModeDto::Internal,
        }
    }
}

impl From<MixModeDto> for MixMode {
    fn from(m: MixModeDto) -> Self {
        match m {
            MixModeDto::External => MixMode::External,
            MixModeDto::Internal => MixMode::Internal,
        }
    }
}

/// Audio settings chosen by the user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AudioSettings {
    pub backend: BackendDto,
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
            backend: BackendDto::Wasapi,
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
    pub cues: Vec<CueDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CueDto {
    /// Hot cue slot, `None` for the main cue.
    pub hot: Option<u8>,
    pub position_s: f64,
    pub loop_length_s: Option<f64>,
    /// 0xRRGGBB
    pub color: u32,
    pub label: String,
}

impl From<&Cue> for CueDto {
    fn from(c: &Cue) -> Self {
        let hot = match c.slot {
            CueSlot::Main => None,
            CueSlot::Hot(n) => Some(n),
        };
        Self {
            hot,
            position_s: c.position_s,
            loop_length_s: c.loop_length_s,
            color: c.color,
            label: c.label.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum VinylModeDto {
    #[default]
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
    pub cue: bool,
    pub vinyl_enabled: bool,
    pub vinyl_mode: VinylModeDto,
    pub lead_in_s: f64,
    pub swap_channels: bool,
}

/// Settings the backend applies to the engine (the UI mirrors them at startup).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MixerStateDto {
    pub decks: Vec<DeckSettingsDto>,
    pub crossfader: f32,
    /// The mode the user chose.
    pub mode: MixModeDto,
    /// The mode in use: an external mixer needs an output pair per deck.
    pub mode_in_use: Option<MixModeDto>,
    pub cue_mix: f32,
    pub headphone_gain: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VinylDto {
    pub enabled: bool,
    pub mode: VinylModeDto,
    /// "none", "bounce" or "ok"
    pub signal: &'static str,
    /// Timecode carrier level (full scale = 1.0).
    pub level: f32,
    pub record_speed: f64,
    pub record_position_s: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncDto {
    pub locked: bool,
    pub leader: bool,
    pub rate_offset: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckStatusDto {
    pub loaded: bool,
    pub playing: bool,
    pub position_s: f64,
    pub duration_s: f64,
    pub speed: f64,
    pub bpm: Option<f64>,
    pub beat_phase: Option<f64>,
    pub sync: SyncDto,
    pub looping: LoopDto,
    pub peak: f32,
    pub cue: bool,
    pub slip_mode: bool,
    pub slip_position_s: Option<f64>,
    pub vinyl: VinylDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopDto {
    pub start_s: Option<f64>,
    pub end_s: Option<f64>,
    pub active: bool,
}

impl LoopDto {
    fn new(region: Option<LoopRegion>, active: bool) -> Self {
        Self { start_s: region.map(|r| r.start_s), end_s: region.map(|r| r.end_s), active }
    }
}

impl From<&DeckStatus> for DeckStatusDto {
    fn from(d: &DeckStatus) -> Self {
        Self {
            loaded: d.loaded,
            playing: d.playing,
            position_s: d.position_s,
            duration_s: d.duration_s,
            speed: d.speed,
            bpm: d.bpm,
            beat_phase: d.beat_phase,
            sync: SyncDto { locked: d.sync.locked, leader: d.sync.leader, rate_offset: d.sync.rate_offset },
            looping: LoopDto::new(d.looping.region, d.looping.active),
            peak: d.peak,
            cue: d.cue,
            slip_mode: d.slip_mode,
            slip_position_s: d.slip_position_s,
            vinyl: VinylDto {
                enabled: d.vinyl.enabled,
                mode: d.vinyl.mode.into(),
                signal: match d.vinyl.signal {
                    SignalState::None => "none",
                    SignalState::Bounce => "bounce",
                    SignalState::Ok => "ok",
                },
                level: d.vinyl.level,
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
    pub health: Option<AudioHealthDto>,
}

/// How well the audio keeps up.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioHealthDto {
    /// Periods processed too late: each one is an audible click.
    pub over_budget: u64,
    pub worst_callback_us: f64,
    pub input_underruns: u64,
    pub device_lost: bool,
}

impl From<&StreamStats> for AudioHealthDto {
    fn from(s: &StreamStats) -> Self {
        Self {
            over_budget: s.over_budget.load(Ordering::Relaxed),
            worst_callback_us: s.max_callback_ns.load(Ordering::Relaxed) as f64 / 1000.0,
            input_underruns: s.input_underruns.load(Ordering::Relaxed),
            device_lost: s.device_lost.load(Ordering::Relaxed),
        }
    }
}

impl StatusDto {
    pub fn from_engine(s: &EngineStatus, stats: &StreamStats) -> Self {
        let health = AudioHealthDto::from(stats);
        Self {
            audio_running: !health.device_lost,
            decks: s.decks[..s.deck_count].iter().map(DeckStatusDto::from).collect(),
            main_peak: s.main_peak,
            health: Some(health),
        }
    }

    pub fn stopped() -> Self {
        Self { audio_running: false, decks: Vec::new(), main_peak: 0.0, health: None }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntryDto {
    pub name: String,
    pub path: String,
    /// What the library knows of a track, `None` until it has been analysed.
    pub details: Option<TrackDetailsDto>,
}

impl DirEntryDto {
    /// A track known by its path (search results, crates).
    pub fn track(path: &str, record: Option<&prisme_library::TrackRecord>) -> Self {
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned());
        Self {
            name: name.unwrap_or_else(|| path.to_string()),
            path: path.to_string(),
            details: record.map(TrackDetailsDto::from),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrateDto {
    pub id: i64,
    pub name: String,
    pub track_count: usize,
}

impl From<prisme_library::Crate> for CrateDto {
    fn from(c: prisme_library::Crate) -> Self {
        Self { id: c.id.0, name: c.name, track_count: c.track_count }
    }
}

/// Analysis and tags of a track as the library shows them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackDetailsDto {
    pub bpm: Option<f64>,
    pub key: Option<KeyDto>,
    pub tags: TagsDto,
    /// The file could not be decoded.
    pub failed: bool,
}

impl From<&prisme_library::TrackRecord> for TrackDetailsDto {
    fn from(r: &prisme_library::TrackRecord) -> Self {
        Self {
            bpm: r.analysis.tempo.map(|t| t.bpm),
            key: r.analysis.key.as_ref().map(KeyDto::from),
            tags: TagsDto::from(&r.tags),
            failed: r.error.is_some(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagsDto {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub year: Option<u16>,
}

impl From<&prisme_media::TrackTags> for TagsDto {
    fn from(t: &prisme_media::TrackTags) -> Self {
        Self {
            title: t.title.clone(),
            artist: t.artist.clone(),
            album: t.album.clone(),
            genre: t.genre.clone(),
            year: t.year,
        }
    }
}

/// Payload of the `track-analyzed` event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackAnalyzedDto {
    pub path: String,
    pub details: TrackDetailsDto,
}

/// Payload of the `library-progress` event.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressDto {
    pub total: usize,
    pub done: usize,
    pub failed: usize,
    pub running: bool,
}

impl From<prisme_library::AnalysisProgress> for ProgressDto {
    fn from(p: prisme_library::AnalysisProgress) -> Self {
        Self { total: p.total, done: p.done, failed: p.failed, running: p.running }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirListingDto {
    pub path: String,
    pub parent: Option<String>,
    pub folders: Vec<DirEntryDto>,
    pub tracks: Vec<DirEntryDto>,
}
