//! Data exchanged with the frontend (camelCase JSON).

use crate::auto_dj::machine::{Reason, State, Status, Takeover};
use crate::auto_dj::plan::FadeLength;
use crate::commands::mixer::mixer_state_of;
use crate::state::{lock, AppState, LoadedTrack, Section};
use prisme_actions::pads::playing_loop;
use prisme_actions::perform::DeckPerform;
pub use prisme_actions::stems::{Separation as SeparationDto, StemMix as StemMixDto};
use prisme_actions::view::View;
use prisme_actions::{light, screen};
use prisme_audio_io::{Backend, StreamStats};
use prisme_engine::{
    InputChannels, LiveSource, LoopStatus, MixMode, Rpm, StemTracks, VinylMode, FX_SLOTS, MAX_DECKS,
};
use prisme_library::{Cue, CueSlot};
use prisme_timecode::{SignalState, TimecodeFormat};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackendDto {
    Wasapi,
    Asio,
    CoreAudio,
    Alsa,
    Jack,
    Aaudio,
    Usb,
}

impl Default for BackendDto {
    fn default() -> Self {
        Backend::SYSTEM.into()
    }
}

impl From<BackendDto> for Backend {
    fn from(b: BackendDto) -> Self {
        match b {
            BackendDto::Wasapi => Backend::Wasapi,
            BackendDto::Asio => Backend::Asio,
            BackendDto::CoreAudio => Backend::CoreAudio,
            BackendDto::Alsa => Backend::Alsa,
            BackendDto::Jack => Backend::Jack,
            BackendDto::Aaudio => Backend::AAudio,
            BackendDto::Usb => Backend::Usb,
        }
    }
}

impl From<Backend> for BackendDto {
    fn from(b: Backend) -> Self {
        match b {
            Backend::Wasapi => BackendDto::Wasapi,
            Backend::Asio => BackendDto::Asio,
            Backend::CoreAudio => BackendDto::CoreAudio,
            Backend::Alsa => BackendDto::Alsa,
            Backend::Jack => BackendDto::Jack,
            Backend::AAudio => BackendDto::Aaudio,
            Backend::Usb => BackendDto::Usb,
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

/// Where the headphones go: those of the software mixer, or with an external mixer those of
/// the preview.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HeadphonesOutputDto {
    /// Outputs 3-4 of a DJ interface (four outputs, or ASIO); nowhere on a computer's
    /// surround outputs, where 3-4 are the centre and the subwoofer of the room.
    #[default]
    Auto,
    Off,
    /// The pair from output `first` (0-based).
    Pair {
        first: usize,
    },
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

impl From<prisme_control::Band> for BandDto {
    fn from(b: prisme_control::Band) -> Self {
        match b {
            prisme_control::Band::Low => Self::Low,
            prisme_control::Band::Mid => Self::Mid,
            prisme_control::Band::High => Self::High,
        }
    }
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

impl From<EffectKindDto> for prisme_control::Effect {
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

impl From<prisme_control::Effect> for EffectKindDto {
    fn from(e: prisme_control::Effect) -> Self {
        use prisme_control::Effect;
        match e {
            Effect::Echo => Self::Echo,
            Effect::PingPong => Self::PingPong,
            Effect::Reverb => Self::Reverb,
            Effect::Flanger => Self::Flanger,
            Effect::Phaser => Self::Phaser,
            Effect::Filter => Self::Filter,
            Effect::Gate => Self::Gate,
            Effect::Bitcrusher => Self::Bitcrusher,
        }
    }
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

/// A recording ended: where it went, and what could not be written.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingDto {
    /// Its first file.
    pub path: String,
    /// Every file it was written in: the first, then `… 2.wav` past 4 GB or an audio restart.
    pub files: Vec<String>,
    pub seconds: f64,
    /// Tracks in its tracklist.
    pub tracks: usize,
    /// The tracklist written beside it, if it could be.
    pub tracklist: Option<String>,
    /// Why the audio was not written whole (a disk full, a drive gone), if it was not.
    pub failure: Option<String>,
}

/// A slot of a deck's effect chain.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FxDto {
    pub kind: EffectKindDto,
    pub enabled: bool,
    /// Level of the effect, 0..1.
    pub mix: f32,
    /// The effect's main knob, 0..1.
    pub amount: f32,
    /// The effect's second knob (tone, resonance...), 0..1; written before it, the middle,
    /// where the effect sounds as it did without one.
    #[serde(default = "param2_default")]
    pub param2: f32,
    /// The effect's period in beats.
    pub beats: f32,
}

fn param2_default() -> f32 {
    prisme_engine::PARAM2_DEFAULT
}

impl Default for FxDto {
    fn default() -> Self {
        Self {
            kind: EffectKindDto::Echo,
            enabled: false,
            mix: 0.5,
            amount: 0.5,
            param2: param2_default(),
            beats: 1.0,
        }
    }
}

/// A deck's beat effects, in series.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", from = "FxChainFile")]
pub struct FxChainDto {
    pub slots: [FxDto; FX_SLOTS],
    /// The whole chain on, each slot then as its own switch says; off, all silent.
    pub enabled: bool,
    /// The part of the track the chain is on, where separated; none: the whole deck.
    pub part: Option<FxPartDto>,
}

impl Default for FxChainDto {
    fn default() -> Self {
        Self { slots: Default::default(), enabled: true, part: None }
    }
}

fn enabled_default() -> bool {
    true
}

/// A chain as written, or a single effect as written before the chain: its first slot.
#[derive(Deserialize)]
#[serde(untagged)]
enum FxChainFile {
    Chain {
        slots: [FxDto; FX_SLOTS],
        #[serde(default = "enabled_default")]
        enabled: bool,
        #[serde(default)]
        part: Option<FxPartDto>,
    },
    Single {
        #[serde(flatten)]
        slot: FxDto,
        #[serde(default)]
        part: Option<FxPartDto>,
    },
}

impl From<FxChainFile> for FxChainDto {
    fn from(file: FxChainFile) -> Self {
        match file {
            FxChainFile::Chain { slots, enabled, part } => Self { slots, enabled, part },
            FxChainFile::Single { slot, part } => {
                let mut slots = [FxDto::default(); FX_SLOTS];
                slots[0] = slot;
                Self { slots, enabled: true, part }
            }
        }
    }
}

/// A part of a track an effect can be on alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FxPartDto {
    Vocals,
    Drums,
    Bass,
    /// What no stem holds (the melody).
    Other,
    /// Everything but the vocals.
    Instrumental,
}

impl FxPartDto {
    /// The parts the engine puts the effect on for `part` (none: the whole deck).
    pub fn parts(part: Option<Self>) -> prisme_engine::Parts {
        use prisme_engine::{Parts, Stem};
        match part {
            None => Parts::NONE,
            Some(Self::Vocals) => Parts::of(Stem::Vocals),
            Some(Self::Drums) => Parts::of(Stem::Drums),
            Some(Self::Bass) => Parts::of(Stem::Bass),
            Some(Self::Other) => Parts::OTHER,
            Some(Self::Instrumental) => Parts::of(Stem::Drums).and(Parts::of(Stem::Bass)).and(Parts::OTHER),
        }
    }
}

impl From<prisme_control::StemPart> for FxPartDto {
    fn from(part: prisme_control::StemPart) -> Self {
        use prisme_control::StemPart;
        match part {
            StemPart::Vocals => Self::Vocals,
            StemPart::Drums => Self::Drums,
            StemPart::Bass => Self::Bass,
            StemPart::Melody => Self::Other,
            StemPart::Instrumental => Self::Instrumental,
        }
    }
}

impl From<FxPartDto> for prisme_control::StemPart {
    fn from(part: FxPartDto) -> Self {
        match part {
            FxPartDto::Vocals => Self::Vocals,
            FxPartDto::Drums => Self::Drums,
            FxPartDto::Bass => Self::Bass,
            FxPartDto::Other => Self::Melody,
            FxPartDto::Instrumental => Self::Instrumental,
        }
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
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AudioSettings {
    pub backend: BackendDto,
    pub output_device: Option<String>,
    /// Turntable input (DVS interface). None = no turntables.
    pub input_device: Option<String>,
    /// Feed deck 1 with a simulated turntable instead of the input.
    pub simulate: bool,
    pub buffer_frames: Option<u32>,
    /// The record of every turntable, in files from before each had its own (see
    /// [`Preferences::settle`](crate::preferences::Preferences::settle)).
    #[serde(rename = "timecodeFormat", skip_serializing)]
    pub(crate) timecode_before: Option<String>,
}

impl AudioSettings {
    /// These settings on the computer's default devices: what plays while the chosen ones are
    /// missing (an interface unplugged).
    pub fn on_default_devices(&self) -> Self {
        Self { simulate: self.simulate, ..Self::default() }
    }

    /// Whether `other` chose the same devices.
    pub fn same_devices(&self, other: &Self) -> bool {
        (self.backend, &self.output_device, &self.input_device)
            == (other.backend, &other.output_device, &other.input_device)
    }
}

/// A sound card just plugged in, offered to the DJ with the settings that use it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardOffer {
    pub name: String,
    pub settings: AudioSettings,
}

/// A round trip measured from an output to an input.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyDto {
    pub round_trip_ms: f64,
    pub sample_rate: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioInfo {
    pub backend: BackendDto,
    pub device: String,
    pub sample_rate: u32,
    pub output_channels: usize,
    pub input_channels: usize,
    /// Decks of the engine.
    pub decks: usize,
    pub simulated: bool,
    /// The sound card chosen in the settings while it is missing and the audio plays on the
    /// default devices meanwhile (empty: the first one a driver finds).
    pub missing: Option<String>,
}

impl AudioInfo {
    /// Its decks and input channels: how it lays the decks on its turntables.
    pub fn layout(&self) -> (usize, usize) {
        (self.decks, self.input_channels)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimecodeFormatDto {
    pub id: &'static str,
    pub maker: &'static str,
    pub record: &'static str,
    pub side: Option<&'static str>,
    pub lead_in_s: f64,
}

impl From<&'static TimecodeFormat> for TimecodeFormatDto {
    fn from(f: &'static TimecodeFormat) -> Self {
        Self { id: f.id, maker: f.maker, record: f.record, side: f.side, lead_in_s: f.lead_in_s }
    }
}

/// Slices of a waveform as sent to the UI, in binary (as JSON, a track's waveform is several
/// hundred kilobytes of numbers): the first slice sent, the slices of the whole track and
/// `slices_per_second` (u32 each, little-endian), then the peak, low, mid and high bands of
/// the slices `range`, one byte per slice each.
pub fn waveform_bytes(
    w: &prisme_analysis::Waveform,
    range: std::ops::Range<usize>,
    slices: usize,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(12 + 4 * range.len());
    for value in [range.start as u32, slices as u32, w.slices_per_second] {
        bytes.extend(value.to_le_bytes());
    }
    for band in [&w.peak, &w.low, &w.mid, &w.high] {
        bytes.extend_from_slice(&band[range.clone()]);
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyDto {
    /// "Am", "F#"...
    pub name: &'static str,
    /// "8A", "2B"...
    pub camelot: &'static str,
}

impl From<prisme_analysis::Key> for KeyDto {
    fn from(key: prisme_analysis::Key) -> Self {
        Self { name: key.name(), camelot: key.camelot() }
    }
}

impl From<&prisme_analysis::KeyEstimate> for KeyDto {
    fn from(k: &prisme_analysis::KeyEstimate) -> Self {
        k.key.into()
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
    pub tempo: Option<TempoDto>,
    pub key: Option<KeyDto>,
    pub grid_edited: bool,
    pub key_edited: bool,
    pub cues: Vec<CueDto>,
    /// Semitones the deck transposes the track by.
    pub key_shift: i8,
    #[serde(flatten)]
    pub stems: DeckStemsDto,
    pub stem_mix: StemMixDto,
}

/// The stems stored next to `track`: how far it is separated.
pub fn stored_separation(track: &std::path::Path) -> SeparationDto {
    SeparationDto::of(|stem| prisme_stems::track::has_stem(track, stem.name()))
}

/// The stems on a deck, which it can mute.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckStemsDto {
    pub separation: SeparationDto,
    /// Separated in parts only: being separated, or a separation stopped short.
    pub partial: bool,
}

impl DeckStemsDto {
    pub fn of(stems: &StemTracks) -> Self {
        Self {
            separation: SeparationDto::of(|stem| stems[stem as usize].is_some()),
            partial: stems.iter().flatten().any(|s| !s.is_complete()),
        }
    }
}

/// The separation queue.
#[derive(Debug, Clone, Default, Serialize)]
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
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatusDto {
    /// `vocals` or `drums-bass`.
    pub id: &'static str,
    pub installed: bool,
    pub downloading: Option<f64>,
    pub error: Option<String>,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StemsStatusDto {
    pub models: Vec<ModelStatusDto>,
    pub progress: StemsProgressDto,
}

/// A track whose stems were just stored.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeparatedDto {
    pub path: String,
    pub separation: SeparationDto,
}

/// What became known of a loaded track after it was put on its deck.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckTrackUpdateDto {
    pub deck: usize,
    pub load_id: u64,
    pub analysis: Option<TrackAnalysisDto>,
    /// The track's separated stems reached the deck, or its separation stopped short.
    pub stems: Option<DeckStemsDto>,
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

/// A setting of a deck's vinyl control, changed alone: how the record drives the deck, how fast
/// it turns, where on it the track starts (`None`: where the record's own lead-in ends). It is
/// put on and off by an action of the deck (`deckN.vinyl`).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", tag = "setting", content = "value")]
pub enum VinylChangeDto {
    Mode(VinylModeDto),
    Rpm(RpmDto),
    LeadIn(Option<f64>),
}

/// Where a deck's vinyl settings are kept: whether it is on, its mode, speed and lead-in.
pub type VinylSettings<'a> = (&'a mut bool, &'a mut VinylModeDto, &'a mut RpmDto, &'a mut Option<f64>);

impl VinylChangeDto {
    /// Its setting changed among `settings`.
    pub fn apply(self, (_, mode, rpm, lead_in_s): VinylSettings) {
        match self {
            VinylChangeDto::Mode(m) => *mode = m,
            VinylChangeDto::Rpm(r) => *rpm = r,
            VinylChangeDto::LeadIn(s) => *lead_in_s = s,
        }
    }
}

/// The speed the record turns at when the track plays at its own speed.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum RpmDto {
    #[default]
    #[serde(rename = "33")]
    Rpm33,
    #[serde(rename = "45")]
    Rpm45,
}

impl From<RpmDto> for Rpm {
    fn from(r: RpmDto) -> Self {
        match r {
            RpmDto::Rpm33 => Rpm::Rpm33,
            RpmDto::Rpm45 => Rpm::Rpm45,
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
    pub rpm: RpmDto,
    /// Chosen, else the record's own.
    pub lead_in_s: Option<f64>,
    /// The record's own: where its track starts at its speed (s of music).
    pub record_lead_in_s: f64,
    pub swap_channels: bool,
    /// The record on its turntable (a timecode format's id).
    pub timecode: &'static str,
    pub key_lock: bool,
    /// Software mixer: the channel fader starts and stops the deck.
    pub fader_start: bool,
    /// The range of its tempo fader: its own, else the general one.
    pub tempo_range: f64,
    pub strip: ChannelStripDto,
    pub fx: FxChainDto,
    /// Name of the sound in each sampler slot.
    pub samples: Vec<Option<String>>,
    pub sampler_gain: f32,
    /// Its size in beats: what its jumps go by, what its sized loop lasts.
    pub size_beats: f64,
    /// The deck plays its input instead of its track.
    pub passthrough: bool,
    /// Its side of the crossfader.
    pub side: CrossfaderSideDto,
    /// A touch of its controller's jog wheel holds the record (else the wheel only nudges).
    pub jog_scratch: bool,
    /// At the end, the track goes on from its start.
    pub repeat: bool,
    /// Silenced in the master, after its fader (still heard in the headphones).
    pub mute: bool,
}

/// The side of the crossfader a deck is on, or neither (heard whatever the crossfader).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CrossfaderSideDto {
    Left,
    Center,
    Right,
}

impl CrossfaderSideDto {
    /// A and C on the left, B and D on the right.
    pub fn of_deck(deck: usize) -> Self {
        if deck.is_multiple_of(2) {
            Self::Left
        } else {
            Self::Right
        }
    }
}

impl From<CrossfaderSideDto> for prisme_control::Side {
    fn from(side: CrossfaderSideDto) -> Self {
        match side {
            CrossfaderSideDto::Left => Self::Left,
            CrossfaderSideDto::Center => Self::Center,
            CrossfaderSideDto::Right => Self::Right,
        }
    }
}

impl From<prisme_control::Side> for CrossfaderSideDto {
    fn from(side: prisme_control::Side) -> Self {
        match side {
            prisme_control::Side::Left => Self::Left,
            prisme_control::Side::Center => Self::Center,
            prisme_control::Side::Right => Self::Right,
        }
    }
}

impl From<CrossfaderSideDto> for prisme_engine::CrossfaderSide {
    fn from(side: CrossfaderSideDto) -> Self {
        match side {
            CrossfaderSideDto::Left => Self::Left,
            CrossfaderSideDto::Center => Self::Center,
            CrossfaderSideDto::Right => Self::Right,
        }
    }
}

/// The input channels a live source comes in on: one, heard in the middle, or a stereo pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputChannelsDto {
    /// The first (0-based).
    pub first: usize,
    pub stereo: bool,
}

impl InputChannelsDto {
    pub fn channels(self) -> impl Iterator<Item = usize> {
        self.first..=self.first + usize::from(self.stereo)
    }
}

impl From<InputChannelsDto> for InputChannels {
    fn from(c: InputChannelsDto) -> Self {
        Self { first: c.first, stereo: c.stereo }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LiveSourceDto {
    Mic,
    Aux,
}

impl LiveSourceDto {
    pub const ALL: [LiveSourceDto; 2] = [LiveSourceDto::Mic, LiveSourceDto::Aux];
}

impl From<prisme_actions::switch::Live> for LiveSourceDto {
    fn from(live: prisme_actions::switch::Live) -> Self {
        match live {
            prisme_actions::switch::Live::Mic => LiveSourceDto::Mic,
            prisme_actions::switch::Live::Aux => LiveSourceDto::Aux,
        }
    }
}

impl From<LiveSourceDto> for LiveSource {
    fn from(s: LiveSourceDto) -> Self {
        match s {
            LiveSourceDto::Mic => LiveSource::Mic,
            LiveSourceDto::Aux => LiveSource::Aux,
        }
    }
}

/// A live source of the software mixer as the backend knows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStateDto {
    /// The input channels chosen.
    pub input: Option<InputChannelsDto>,
    /// They are on the running device, left free by the turntables.
    pub in_use: bool,
    pub level: f32,
    pub on: bool,
    pub cue: bool,
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
    pub headphones: HeadphonesOutputDto,
    /// The first output of the headphones in use, if they are heard: with an external mixer,
    /// a pair no deck uses, for the preview alone.
    pub headphones_in_use: Option<usize>,
    /// Outputs of the running device.
    pub output_channels: Option<usize>,
    pub cue_mix: f32,
    /// Headphones: the cue on the left and the master on the right.
    pub split_cue: bool,
    pub headphone_gain: f32,
    pub crossfader_curve: CrossfaderCurveDto,
    /// The crossfader's sides swapped.
    pub crossfader_reverse: bool,
    pub limiter: bool,
    pub main_gain: f32,
    pub balance: f32,
    /// The first output of the booth chosen, and the one in use if it is heard.
    pub booth: Option<usize>,
    pub booth_in_use: Option<usize>,
    pub booth_gain: f32,
    pub mic: LiveStateDto,
    pub aux: LiveStateDto,
    pub talkover: bool,
    pub talkover_depth_db: f32,
    /// The inputs of the running device the turntables leave free (0-based).
    pub free_inputs: Vec<usize>,
    /// What was recorded last.
    pub record_source: Option<RecordSourceDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VinylDto {
    pub enabled: bool,
    pub mode: VinylModeDto,
    /// The needle reached the end of the record: the deck plays on by itself.
    pub run_out: bool,
    /// The record has read backwards at play speed for a while: crossed channels?
    pub crossed: bool,
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
    pub state: screen::Sync,
    pub leader: bool,
    /// What its SYNC key does.
    pub key: screen::SyncKey,
}

/// A deck as the engine has it and the screen shows it at a frame.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckStatusDto {
    pub loaded: bool,
    pub playing: bool,
    pub position_s: f64,
    pub duration_s: f64,
    pub speed: f64,
    /// The rate it plays at: its tempo, gestures excluded, or its fader's without a track.
    pub rate: f64,
    /// Its tempo, gestures excluded (needs a beat grid).
    pub bpm: Option<f64>,
    pub beat_phase: Option<f64>,
    pub heard_key: Option<KeyDto>,
    pub mixed_with: usize,
    /// The DJ hears or holds its track: a load would replace it.
    pub plays: bool,
    pub remaining_s: Option<f64>,
    /// The end of its track near (on), or soon.
    pub ending: light::State,
    /// Who drives it.
    pub mode: screen::Drive,
    pub tempo: screen::TempoFader,
    pub sync: SyncDto,
    /// Its loops, jumps and pads may move its track.
    pub can_move: bool,
    /// Its grid may move onto the beats of the deck it is mixed with.
    pub can_align_grid: bool,
    /// Its key may be moved to mix with the one heard on the deck it is mixed with.
    pub can_sync_key: bool,
    pub lights: DeckLightsDto,
    pub looping: LoopDto,
    pub peak: f32,
    pub cue: bool,
    pub slip_mode: bool,
    /// The reverse is on: the track plays backwards.
    pub reverse: bool,
    pub slip_position_s: Option<f64>,
    pub vinyl: VinylDto,
    /// Sampler slots playing (bit per slot).
    pub samples_playing: u16,
    /// The parts of its separated track, a knob each.
    pub stems: Vec<StemKnobDto>,
}

/// What a deck's keys and pads light by the controllers' rules.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckLightsDto {
    /// Its CUE: on at the cue point, paused stopped elsewhere.
    pub cue: light::State,
    /// The slice of its slicer it plays in.
    pub slice: Option<usize>,
}

/// A part of a deck's separated track, as its knob shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StemKnobDto {
    pub part: FxPartDto,
    pub level: f32,
    pub on: bool,
    /// Its change waits for the next bar.
    pub waiting: bool,
    /// Separated where the deck plays.
    pub ready: bool,
}

impl From<screen::StemKnob> for StemKnobDto {
    fn from(k: screen::StemKnob) -> Self {
        Self { part: k.part.into(), level: k.level, on: k.on, waiting: k.waiting, ready: k.ready }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopDto {
    pub start_s: Option<f64>,
    pub end_s: Option<f64>,
    /// The beats of the loop playing, as the pads of a loop of that length show it
    /// (`pads::playing_loop`).
    pub beats: Option<f64>,
    pub active: bool,
    /// Start marked by LOOP IN, waiting for LOOP OUT.
    pub loop_in_s: Option<f64>,
}

impl From<LoopStatus> for LoopDto {
    fn from(l: LoopStatus) -> Self {
        Self {
            start_s: l.region.map(|r| r.start_s),
            end_s: l.region.map(|r| r.end_s),
            beats: playing_loop(&l),
            active: l.active,
            loop_in_s: l.loop_in_s,
        }
    }
}

impl DeckStatusDto {
    /// `deck` as `view` shows it.
    fn of(view: &View, deck: usize) -> Self {
        let d = view.deck_status(deck);
        let shown = screen::deck(view, deck);
        Self {
            loaded: d.loaded,
            playing: d.playing,
            position_s: d.position_s,
            duration_s: d.duration_s,
            speed: d.speed,
            rate: shown.rate,
            bpm: d.bpm,
            beat_phase: d.beat_phase,
            heard_key: shown.heard_key.map(KeyDto::from),
            mixed_with: shown.mixed_with,
            plays: shown.plays,
            remaining_s: shown.remaining_s,
            ending: shown.ending,
            mode: shown.drive,
            tempo: shown.tempo,
            sync: SyncDto { state: shown.sync, leader: d.sync.leader, key: shown.sync_key },
            can_move: shown.can_move,
            can_align_grid: shown.can_align_grid,
            can_sync_key: shown.can_sync_key,
            lights: DeckLightsDto { cue: shown.cue, slice: shown.slice },
            looping: d.looping.into(),
            peak: d.peak,
            cue: d.cue,
            slip_mode: d.slip_mode,
            reverse: d.reverse,
            slip_position_s: d.slip_position_s,
            samples_playing: d.samples_playing,
            stems: screen::stem_knobs(view, deck).map(StemKnobDto::from).collect(),
            vinyl: VinylDto {
                enabled: d.vinyl.enabled,
                mode: d.vinyl.mode.into(),
                run_out: d.vinyl.run_out,
                crossed: d.vinyl.crossed,
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
    pub mic_peak: f32,
    pub aux_peak: f32,
    pub health: Option<AudioHealthDto>,
    pub recording: Option<RecordingStatusDto>,
    pub preview: Option<PreviewStatusDto>,
    /// The strip comparing two decks' beats.
    pub phase: screen::PhaseStrip,
    /// What the interface mirrors, when it changed since the status before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub controls: Option<ControlStateDto>,
}

/// What the interface mirrors of the decks, their tracks, the mixer and the DJ's preferences,
/// by section (`state::Section`): those that did not change since last sent are left out.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlStateDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mixer: Option<MixerStateDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantize: Option<bool>,
    /// The range of the tempo faders without one of their own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pitch_range: Option<f64>,
    /// Each deck's track by its load, and what changes of it while it stays there: the
    /// interface asks for one it does not show yet (`loaded_tracks`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracks: Option<Vec<Option<TrackStateDto>>>,
    /// Whether each deck loads a track.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loading: Option<[bool; MAX_DECKS]>,
    /// What each deck's pads hold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub perform: Option<[DeckPadsDto; MAX_DECKS]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<PreviewStateDto>,
}

/// What a deck's pads hold, as the interface shows them.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckPadsDto {
    /// The roll playing, in beats.
    pub roll: Option<f64>,
    /// The fader cut playing, of those held.
    pub cut: Option<usize>,
    /// The slot of its effect chain the effect knobs turn.
    pub fx_focus: Option<usize>,
}

impl From<&DeckPerform> for DeckPadsDto {
    fn from(deck: &DeckPerform) -> Self {
        Self { roll: deck.roll, cut: deck.cuts.acting(), fx_focus: deck.fx_focus }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackStateDto {
    /// The load it is of: one the interface does not show yet is not changed.
    pub load_id: u64,
    pub key_shift: i8,
    pub stem_mix: StemMixDto,
    pub cues: Vec<CueDto>,
}

impl ControlStateDto {
    /// The `sections` of `state`.
    pub fn of(state: &AppState, sections: impl IntoIterator<Item = Section>) -> Self {
        let mut controls = Self::default();
        for section in sections {
            match section {
                Section::Mixer => {
                    let (quantize, pitch_range) =
                        state.preferences.read(|p| (p.dj.quantize, p.dj.pitch_range));
                    controls.mixer = Some(mixer_state_of(state));
                    (controls.quantize, controls.pitch_range) = (Some(quantize), Some(pitch_range));
                }
                Section::Tracks => {
                    let tracks =
                        lock(&state.tracks).iter().map(|t| t.as_ref().map(TrackStateDto::of)).collect();
                    controls.tracks = Some(tracks);
                    controls.loading = Some(state.loading());
                }
                Section::Perform => {
                    controls.perform = Some(lock(&state.perform).decks.each_ref().map(DeckPadsDto::from))
                }
                Section::Preview => controls.preview = Some(lock(&state.preview).clone()),
            }
        }
        controls
    }
}

impl TrackStateDto {
    fn of(track: &LoadedTrack) -> Self {
        let cues = track.cue_dtos();
        TrackStateDto { load_id: track.load_id, key_shift: track.key_shift, stem_mix: track.stem_mix, cues }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewStatusDto {
    pub id: u64,
    pub position_s: f64,
    /// Estimated until the track is all read.
    pub duration_s: f64,
    /// Its audio is not read that far yet: it waits, silent.
    pub waiting: bool,
}

/// What the headphones preview: the track the engine was given, and the file being opened
/// for the preview asked last, if any.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewStateDto {
    pub track: Option<PreviewDto>,
    pub opening: Option<String>,
}

/// A track previewed in the headphones.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewDto {
    /// What the status tells it by.
    pub id: u64,
    pub path: String,
    pub title: String,
    pub artist: Option<String>,
    /// Estimated until the track is all read.
    pub duration_s: f64,
    /// The waveform can be fetched (`preview_waveform`); otherwise a `preview-update` says
    /// when it can.
    pub waveform_ready: bool,
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
    /// Input frames dropped: more had come than the output took.
    pub input_dropped: u64,
    pub device_lost: bool,
    /// The engine failed and could not be restarted: the audio is silent.
    pub engine_failed: bool,
    /// Frames per period, as the driver calls the engine (0 before the first period).
    pub period_frames: u64,
}

impl From<&StreamStats> for AudioHealthDto {
    fn from(s: &StreamStats) -> Self {
        Self {
            over_budget: s.over_budget.load(Ordering::Relaxed),
            worst_callback_us: s.max_callback_ns.load(Ordering::Relaxed) as f64 / 1000.0,
            input_underruns: s.input_underruns.load(Ordering::Relaxed),
            input_dropped: s.input_dropped.load(Ordering::Relaxed),
            device_lost: s.device_lost.load(Ordering::Relaxed),
            engine_failed: s.callback_failed.load(Ordering::Relaxed),
            period_frames: s.frames.load(Ordering::Relaxed) / s.callbacks.load(Ordering::Relaxed).max(1),
        }
    }
}

impl StatusDto {
    /// What `view` shows, its engine running a stream of `stats` at `sample_rate` (none: the
    /// audio is off): the decks in use.
    pub fn of(view: &View, stream: Option<(&StreamStats, u32)>) -> Self {
        let s = &view.status;
        let health = stream.map(|(stats, _)| AudioHealthDto::from(stats));
        let sample_rate = stream.map_or(1.0, |(_, rate)| f64::from(rate));
        Self {
            audio_running: health.as_ref().is_some_and(|h| !h.device_lost),
            decks: (0..view.decks_in_use).map(|d| DeckStatusDto::of(view, d)).collect(),
            main_peak: s.main_peak,
            mic_peak: s.mic_peak,
            aux_peak: s.aux_peak,
            health,
            recording: s.recording.map(|r| RecordingStatusDto {
                seconds: r.frames as f64 / sample_rate,
                dropped_seconds: r.dropped as f64 / sample_rate,
            }),
            preview: s.preview.map(|p| PreviewStatusDto {
                id: p.id,
                position_s: p.position_s,
                duration_s: p.duration_s,
                waiting: p.waiting,
            }),
            phase: screen::phase_strip(view),
            controls: None,
        }
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

/// What an export to rekordbox wrote.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummaryDto {
    pub tracks: usize,
    pub lists: usize,
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
    /// Its stems stored next to it.
    pub separation: SeparationDto,
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
            separation: stored_separation(std::path::Path::new(&r.path)),
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
    /// Held back while a deck plays.
    pub paused: bool,
}

impl From<prisme_library::AnalysisProgress> for ProgressDto {
    fn from(p: prisme_library::AnalysisProgress) -> Self {
        Self { total: p.total, done: p.done, failed: p.failed, running: p.running, paused: p.paused }
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

/// A music folder: the main one, or one added next to it (a USB key, a drive, a sets
/// folder). Payload of the `music-folders-changed` event, as a list, the main one first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicFolderDto {
    pub path: String,
    pub main: bool,
    /// The folder is there now: one on a drive unplugged is not, its tracks kept.
    pub present: bool,
}

/// What holds a [`StoragePlaceDto`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StoragePlaceKind {
    /// The phone's own storage.
    Phone,
    /// A memory card or a USB drive plugged into the phone.
    Removable,
    /// The home folder of the computer.
    Home,
    /// A drive of the computer.
    Drive,
}

/// Where a folder to add to the music folders is looked for first, named by its own folder
/// (`3A2F-1C0B`, `C:`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoragePlaceDto {
    pub kind: StoragePlaceKind,
    pub name: String,
    pub path: String,
}

/// A controller, MIDI or HID.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerDto {
    /// Its MIDI port's name, or the name Prisme gives a HID device.
    pub name: String,
    /// A HID device.
    pub hid: bool,
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
}

/// A profile the DJ may choose for a controller.
#[derive(Debug, Clone, Serialize)]
pub struct OfferedProfileDto {
    pub file: String,
    pub name: String,
}

/// The controller models Prisme knows, and where the DJ's own profiles go.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerProfilesDto {
    pub profiles: Vec<ControllerProfileDto>,
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
    pub variant: bool,
    /// Prisme's model whose profile of the same file name this one, the DJ's own, replaces.
    pub replaces: Option<String>,
}

/// An action the interface performs that a controller drove: a key's value, or an encoder's
/// whole steps.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiActionDto {
    pub target: String,
    pub value: f32,
    /// Its number, which the interface tells with the selection once it applied it
    /// (`set_library_selection`).
    pub number: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AutoDjStateDto {
    Off,
    On,
    Mixing,
    Paused,
}

/// Why the Auto DJ stopped, paused or left a track out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AutoDjReasonDto {
    ExternalMixer,
    Records,
    ListEmpty,
    ListEnded,
    /// The DJ played, stopped or moved a deck (`reasonDeck`).
    Transport,
    Crossfader,
    /// The DJ loaded a deck (`reasonDeck`).
    Load,
    Unloadable,
}

/// The Auto DJ as the interface shows it (`auto-dj`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoDjStatusDto {
    pub state: AutoDjStateDto,
    pub reason: Option<AutoDjReasonDto>,
    pub reason_deck: Option<usize>,
    /// The deck heard.
    pub current: Option<usize>,
    /// The deck being mixed in.
    pub incoming: Option<usize>,
    /// Where on the deck heard the next mix starts (to the hundredth of a second).
    pub mix_at_s: Option<f64>,
    pub fade_beats: u32,
    pub fade_s: f64,
}

impl AutoDjStatusDto {
    pub fn of(status: Status, fade: FadeLength) -> Self {
        let (reason, reason_deck) = match status.reason {
            None => (None, None),
            Some(Reason::ExternalMixer) => (Some(AutoDjReasonDto::ExternalMixer), None),
            Some(Reason::Records) => (Some(AutoDjReasonDto::Records), None),
            Some(Reason::ListEmpty) => (Some(AutoDjReasonDto::ListEmpty), None),
            Some(Reason::ListEnded) => (Some(AutoDjReasonDto::ListEnded), None),
            Some(Reason::Unloadable) => (Some(AutoDjReasonDto::Unloadable), None),
            Some(Reason::TakenOver(Takeover::Transport(deck))) => {
                (Some(AutoDjReasonDto::Transport), Some(deck))
            }
            Some(Reason::TakenOver(Takeover::Load(deck))) => (Some(AutoDjReasonDto::Load), Some(deck)),
            Some(Reason::TakenOver(Takeover::Crossfader)) => (Some(AutoDjReasonDto::Crossfader), None),
        };
        Self {
            state: match status.state {
                State::Off => AutoDjStateDto::Off,
                State::On => AutoDjStateDto::On,
                State::Mixing => AutoDjStateDto::Mixing,
                State::Paused => AutoDjStateDto::Paused,
            },
            reason,
            reason_deck,
            current: status.current,
            incoming: status.incoming,
            mix_at_s: status.mix_at_s.map(|s| (s * 100.0).round() / 100.0),
            fade_beats: fade.beats,
            fade_s: fade.seconds,
        }
    }
}

/// The state of an action whose light the interface tells (`on`, `off`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LightDto {
    pub target: String,
    pub state: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_the_interface_mirrors_moves_the_revision_and_reaches_it_a_read_does_not() {
        let state = AppState::new(prisme_session::JsonFile::in_memory(Default::default()));
        let before = state.revision();
        let pitch = lock(&state.decks)[1].pitch;
        assert_eq!((pitch, state.moved_since(before)), (1.0, vec![]), "read");
        lock(&state.decks)[1].strip.eq[0] = 0.5;
        assert_eq!(state.moved_since(before), [Section::Mixer]);
        let eq = state.revision();
        state.update_preferences(|p| p.dj.quantize = true);
        *lock(&state.crossfader) = -1.0;
        assert_eq!(state.moved_since(eq), [Section::Mixer]);
        let controls = ControlStateDto::of(&state, Section::ALL);
        let mixer = controls.mixer.unwrap();
        assert_eq!(mixer.decks[1].strip.eq[0], 0.5);
        assert_eq!(mixer.crossfader, -1.0);
        assert_eq!(controls.quantize, Some(true));
        assert_eq!(controls.tracks.unwrap().len(), prisme_engine::MAX_DECKS);
    }

    #[test]
    fn a_change_moves_the_revision_of_its_section_alone() {
        let state = AppState::new(prisme_session::JsonFile::in_memory(Default::default()));
        let before = state.revision();
        lock(&state.perform).decks[0].roll = Some(1.0);
        let moved = state.moved_since(before);
        assert_eq!(moved, [Section::Perform]);
        let controls = serde_json::to_value(ControlStateDto::of(&state, moved)).unwrap();
        assert_eq!(
            controls.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["perform"],
            "the rest left out"
        );
    }

    #[test]
    fn the_default_devices_keep_the_simulated_turntable() {
        let chosen = AudioSettings {
            output_device: Some("UMC404HD".into()),
            input_device: Some("UMC404HD".into()),
            simulate: true,
            buffer_frames: Some(128),
            ..AudioSettings::default()
        };
        let fallback = chosen.on_default_devices();
        assert_eq!(
            (fallback.output_device, fallback.input_device, fallback.buffer_frames),
            (None, None, None)
        );
        assert!(fallback.simulate);
        assert!(chosen.same_devices(&AudioSettings { simulate: false, ..chosen.clone() }));
        assert!(!chosen.same_devices(&AudioSettings { output_device: None, ..chosen.clone() }));
    }

    #[test]
    fn a_single_effect_written_before_the_chain_reads_as_its_first_slot() {
        let old = r#"{"kind":"gate","enabled":true,"mix":0.8,"amount":0.3,"beats":2.0,"part":"vocals"}"#;
        let chain: FxChainDto = serde_json::from_str(old).unwrap();
        let gate = FxDto {
            kind: EffectKindDto::Gate,
            enabled: true,
            mix: 0.8,
            amount: 0.3,
            param2: 0.5,
            beats: 2.0,
        };
        assert_eq!(chain.slots, [gate, FxDto::default(), FxDto::default()]);
        assert_eq!(chain.part, Some(FxPartDto::Vocals));
        assert!(chain.enabled);
        let written = serde_json::to_string(&chain).unwrap();
        assert_eq!(serde_json::from_str::<FxChainDto>(&written).unwrap(), chain);
    }

    #[test]
    fn a_chain_written_before_its_switch_and_second_knobs_reads_on_with_them_in_the_middle() {
        let slot = r#"{"kind":"filter","enabled":true,"mix":1.0,"amount":0.7,"beats":4.0}"#;
        let old = format!(r#"{{"slots":[{slot},{slot},{slot}],"part":null}}"#);
        let chain: FxChainDto = serde_json::from_str(&old).unwrap();
        assert!(chain.enabled);
        assert!(chain.slots.iter().all(|s| s.param2 == prisme_engine::PARAM2_DEFAULT && s.amount == 0.7));
        let changed = FxChainDto { enabled: false, ..chain };
        let written = serde_json::to_string(&changed).unwrap();
        assert_eq!(serde_json::from_str::<FxChainDto>(&written).unwrap(), changed);
    }

    #[test]
    fn a_vinyl_setting_is_changed_by_its_name_but_never_put_on() {
        let lead_in: VinylChangeDto = serde_json::from_str(r#"{"setting":"leadIn","value":null}"#).unwrap();
        assert_eq!(lead_in, VinylChangeDto::LeadIn(None));
        let rpm: VinylChangeDto = serde_json::from_str(r#"{"setting":"rpm","value":"45"}"#).unwrap();
        assert_eq!(rpm, VinylChangeDto::Rpm(RpmDto::Rpm45));
        assert!(serde_json::from_str::<VinylChangeDto>(r#"{"setting":"enabled","value":true}"#).is_err());
    }
}
