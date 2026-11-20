//! The mixer: its knobs and switches, the routing, the live sources, the recording.

use crate::choices::{BoothChoice, LiveNote, OutputNote};
use prisme_actions::knobs::MixerKnobs;
use prisme_engine::{InputChannels, LiveSource, MixMode};
use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecordingDto {
    /// Its first file.
    pub path: String,
    /// The name of its first file, and the folder it is in.
    pub file_name: String,
    pub folder: String,
    /// The files it was written in: the first, then `… 2.wav` past 4 GB or an audio restart.
    pub parts: usize,
    pub seconds: f64,
    /// Tracks in its tracklist.
    pub tracks: usize,
    /// The name of the tracklist written beside it, if it could be.
    pub tracklist: Option<String>,
    /// Why the audio was not written whole, if it was not.
    pub failure: Option<RecordingFailureDto>,
}

/// Why a recording was not written whole.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RecordingFailureDto {
    DiskFull,
    /// Its file could not be written: a drive gone, a file locked.
    NotWritten,
    /// Its writer stopped unexpectedly.
    Stopped,
}

impl RecordingDto {
    /// A recording of `parts` files from `first`, its tracklist at `tracklist`.
    pub fn of(first: &std::path::Path, parts: usize, tracklist: Option<&std::path::Path>) -> Self {
        let name = |path: &std::path::Path| path.file_name().map(|n| n.to_string_lossy().into_owned());
        Self {
            path: first.display().to_string(),
            file_name: name(first).unwrap_or_default(),
            folder: first.parent().map(|f| f.display().to_string()).unwrap_or_default(),
            parts,
            seconds: 0.0,
            tracks: 0,
            tracklist: tracklist.and_then(name),
            failure: None,
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
    /// What it can come in on: the inputs the turntables and the other source leave.
    pub choices: Vec<InputChannelsDto>,
    /// Why it is not heard on the running device.
    pub note: Option<LiveNote>,
    pub on: bool,
    pub cue: bool,
}

/// Settings the backend applies to the engine (the UI mirrors them at startup).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MixerStateDto {
    /// The knobs beside the channels and the crossfader, as the screen shows them.
    pub knobs: MixerKnobs,
    /// The mode the user chose.
    pub mode: MixModeDto,
    /// The software mixer is in use (or would be, the audio off).
    pub internal: bool,
    /// An external mixer chosen, the device too small for it: the outputs it needs.
    pub fallback: Option<usize>,
    /// The first output each deck in use plays on to an external mixer (0-based).
    pub deck_outputs: Vec<usize>,
    pub headphones: HeadphonesOutputDto,
    /// The first output of the headphones in use, if they are heard: with an external mixer,
    /// a pair no deck uses, for the preview alone.
    pub headphones_in_use: Option<usize>,
    /// The pairs the headphones can take, with the one chosen (first outputs).
    pub headphones_choices: Vec<usize>,
    /// Where the headphones are heard on the running device.
    pub headphones_note: Option<OutputNote>,
    /// Headphones: the cue on the left and the master on the right.
    pub split_cue: bool,
    pub crossfader_curve: CrossfaderCurveDto,
    /// The crossfader's sides swapped.
    pub crossfader_reverse: bool,
    pub limiter: bool,
    /// The first output of the booth chosen, and the one in use if it is heard.
    pub booth: Option<usize>,
    pub booth_in_use: Option<usize>,
    pub booth_choices: Vec<BoothChoice>,
    /// Where the booth chosen is heard, or why not, on the software mixer of the running device.
    pub booth_note: Option<OutputNote>,
    pub mic: LiveStateDto,
    pub aux: LiveStateDto,
    pub talkover: bool,
    pub talkover_depth_db: f32,
    /// What was recorded last.
    pub record_source: Option<RecordSourceDto>,
    /// What can be recorded on the running device.
    pub record_options: Vec<RecordOptionDto>,
    /// The last recording ended.
    pub last_recording: Option<RecordingDto>,
}

/// A source the Rec menu offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordOptionDto {
    pub source: RecordSourceDto,
    /// A turntable comes into these inputs: its timecode would be recorded.
    pub turntable: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "RecordingStatus"))]
pub struct RecordingStatusDto {
    pub seconds: f64,
    /// Audio the writer could not take in time (lost).
    pub dropped_seconds: f64,
}
