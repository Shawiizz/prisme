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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CrossfaderCurveDto {
    /// Equal power, for blending.
    #[default]
    Smooth,
    /// Cuts only at the very ends, for scratching.
    Sharp,
}

impl From<CrossfaderCurveDto> for prisme_engine::CrossfaderCurve {
    fn from(c: CrossfaderCurveDto) -> Self {
        match c {
            CrossfaderCurveDto::Smooth => Self::Smooth,
            CrossfaderCurveDto::Sharp => Self::Sharp,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BandDto {
    Low,
    Mid,
    High,
}

impl BandDto {
    pub const ALL: [BandDto; 3] = [BandDto::Low, BandDto::Mid, BandDto::High];
}

impl From<BandDto> for prisme_engine::Band {
    fn from(b: BandDto) -> Self {
        match b {
            BandDto::Low => Self::Low,
            BandDto::Mid => Self::Mid,
            BandDto::High => Self::High,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EffectKindDto {
    #[default]
    Echo,
    PingPong,
    Reverb,
    Flanger,
    Phaser,
    Filter,
    Gate,
    Bitcrusher,
}

impl From<EffectKindDto> for prisme_engine::EffectKind {
    fn from(k: EffectKindDto) -> Self {
        match k {
            EffectKindDto::Echo => Self::Echo,
            EffectKindDto::PingPong => Self::PingPong,
            EffectKindDto::Reverb => Self::Reverb,
            EffectKindDto::Flanger => Self::Flanger,
            EffectKindDto::Phaser => Self::Phaser,
            EffectKindDto::Filter => Self::Filter,
            EffectKindDto::Gate => Self::Gate,
            EffectKindDto::Bitcrusher => Self::Bitcrusher,
        }
    }
}

/// What a recording takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RecordSourceDto {
    /// The software mixer's master.
    Master,
    /// An input pair, from its first channel (0-based).
    Input { first: usize },
}

impl From<RecordSourceDto> for prisme_engine::RecordSource {
    fn from(s: RecordSourceDto) -> Self {
        match s {
            RecordSourceDto::Master => Self::Master,
            RecordSourceDto::Input { first } => Self::Input(first),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingDto {
    pub path: String,
    pub seconds: f64,
    /// Tracks in its tracklist.
    pub tracks: usize,
}

/// A deck's beat effect.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FxDto {
    pub kind: EffectKindDto,
    pub enabled: bool,
    /// Level of the effect, 0..1.
    pub mix: f32,
    /// The effect's main knob, 0..1.
    pub amount: f32,
    /// The effect's period in beats.
    pub beats: f32,
}

impl Default for FxDto {
    fn default() -> Self {
        Self { kind: EffectKindDto::Echo, enabled: false, mix: 0.5, amount: 0.5, beats: 1.0 }
    }
}

/// A channel of the software mixer, before its fader. Gains are linear.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelStripDto {
    pub trim: f32,
    /// Low, mid, high.
    pub eq: [f32; 3],
    pub kills: [bool; 3],
    /// -1 = low-pass closed, 0 = off, 1 = high-pass open.
    pub filter: f32,
}

impl Default for ChannelStripDto {
    fn default() -> Self {
        Self { trim: 1.0, eq: [1.0; 3], kills: [false; 3], filter: 0.0 }
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

/// A waveform as sent to the UI, in binary (as JSON, a track's waveform is several
/// hundred kilobytes of numbers): `slices_per_second` (u32, little-endian), then the
/// peak, low, mid and high bands, one byte per slice each.
pub fn waveform_bytes(w: &prisme_analysis::Waveform) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(4 + 4 * w.len());
    bytes.extend(w.slices_per_second.to_le_bytes());
    for band in [&w.peak, &w.low, &w.mid, &w.high] {
        bytes.extend_from_slice(band);
    }
    bytes
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
    /// Tells this load from another of the same file (see `deck-track-update`).
    pub load_id: u64,
    pub path: String,
    pub title: String,
    pub artist: Option<String>,
    pub duration_s: f64,
    pub sample_rate: u32,
    /// The waveform can be fetched (`deck_waveform`); otherwise a `deck-track-update`
    /// says when it can.
    pub waveform_ready: bool,
    pub tempo: Option<TempoDto>,
    pub key: Option<KeyDto>,
    pub grid_edited: bool,
    pub key_edited: bool,
    pub cues: Vec<CueDto>,
}

/// What became known of a loaded track after it was put on its deck.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckTrackUpdateDto {
    pub deck: usize,
    pub load_id: u64,
    /// The waveform was computed: it can be fetched (`deck_waveform`).
    pub waveform_ready: bool,
    pub analysis: Option<TrackAnalysisDto>,
}

/// Tempo and key of a loaded track, after a correction or its analysis.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackAnalysisDto {
    pub tempo: Option<TempoDto>,
    pub key: Option<KeyDto>,
    pub grid_edited: bool,
    pub key_edited: bool,
}

impl From<&crate::state::LoadedTrack> for TrackAnalysisDto {
    fn from(track: &crate::state::LoadedTrack) -> Self {
        let analysis = track.analysis();
        Self {
            tempo: analysis.tempo.as_ref().map(TempoDto::from),
            key: analysis.key.as_ref().map(KeyDto::from),
            grid_edited: track.edits.grid.is_some(),
            key_edited: track.edits.key.is_some(),
        }
    }
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
    pub key_lock: bool,
    pub strip: ChannelStripDto,
    pub fx: FxDto,
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
    pub crossfader_curve: CrossfaderCurveDto,
    pub limiter: bool,
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
    pub recording: Option<RecordingStatusDto>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStatusDto {
    pub seconds: f64,
    /// Audio the writer could not take in time (lost).
    pub dropped_seconds: f64,
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
    pub fn from_engine(s: &EngineStatus, stats: &StreamStats, sample_rate: u32) -> Self {
        let health = AudioHealthDto::from(stats);
        Self {
            audio_running: !health.device_lost,
            decks: s.decks[..s.deck_count].iter().map(DeckStatusDto::from).collect(),
            main_peak: s.main_peak,
            health: Some(health),
            recording: s.recording.map(|r| RecordingStatusDto {
                seconds: r.frames as f64 / sample_rate as f64,
                dropped_seconds: r.dropped as f64 / sample_rate as f64,
            }),
        }
    }

    pub fn stopped() -> Self {
        Self { audio_running: false, decks: Vec::new(), main_peak: 0.0, health: None, recording: None }
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
    /// The rules of a smart crate.
    pub rules: Option<prisme_library::SmartRules>,
    pub track_count: usize,
}

/// What an import brought.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummaryDto {
    pub tracks: usize,
    pub cues: usize,
    pub grids: usize,
    pub crates: usize,
    pub missing: usize,
}

impl From<prisme_import::ImportSummary> for ImportSummaryDto {
    fn from(s: prisme_import::ImportSummary) -> Self {
        Self { tracks: s.tracks, cues: s.cues, grids: s.grids, crates: s.crates, missing: s.missing }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySessionDto {
    pub id: i64,
    pub started_at_ms: i64,
    pub play_count: usize,
}

impl From<prisme_library::HistorySession> for HistorySessionDto {
    fn from(s: prisme_library::HistorySession) -> Self {
        Self { id: s.id.0, started_at_ms: s.started_at_ms, play_count: s.play_count }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPlayDto {
    #[serde(flatten)]
    pub track: DirEntryDto,
    pub deck: usize,
    pub played_at_ms: i64,
}

impl From<&prisme_library::HistoryPlay> for HistoryPlayDto {
    fn from(p: &prisme_library::HistoryPlay) -> Self {
        Self {
            track: DirEntryDto::track(&p.path, p.record.as_ref()),
            deck: p.deck,
            played_at_ms: p.played_at_ms,
        }
    }
}

impl From<prisme_library::Crate> for CrateDto {
    fn from(c: prisme_library::Crate) -> Self {
        Self { id: c.id.0, name: c.name, rules: c.rules, track_count: c.track_count }
    }
}

/// Analysis and tags of a track as the library shows them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackDetailsDto {
    pub bpm: Option<f64>,
    pub key: Option<KeyDto>,
    pub tags: TagsDto,
    /// 0xRRGGBB
    pub color: Option<u32>,
    /// 0 (not rated) to 5.
    pub rating: u8,
    pub play_count: usize,
    /// The file could not be decoded.
    pub failed: bool,
}

impl From<&prisme_library::TrackRecord> for TrackDetailsDto {
    fn from(r: &prisme_library::TrackRecord) -> Self {
        let analysis = r.effective_analysis();
        Self {
            bpm: analysis.tempo.map(|t| t.bpm),
            key: analysis.key.as_ref().map(KeyDto::from),
            tags: TagsDto::from(&r.tags),
            color: r.marks.color,
            rating: r.marks.rating,
            play_count: r.play_count,
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
