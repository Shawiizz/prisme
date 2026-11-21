//! The records and how they drive the decks.

use prisme_engine::{Rpm, VinylMode};
use prisme_timecode::SignalState;
use prisme_timecode::TimecodeFormat;
use serde::{Deserialize, Serialize};

/// The timecode records of a maker.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "TimecodeMaker"))]
pub struct TimecodeMakerDto {
    pub name: &'static str,
    pub records: Vec<TimecodeRecordDto>,
}

/// A timecode record (or CD), a format for each side when its sides differ.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "TimecodeRecord"))]
pub struct TimecodeRecordDto {
    pub name: &'static str,
    pub sides: Vec<TimecodeFormatDto>,
}

impl TimecodeMakerDto {
    /// `formats` by maker, then record, in their order.
    pub fn of(formats: &'static [TimecodeFormat]) -> Vec<Self> {
        let mut makers: Vec<Self> = Vec::new();
        for format in formats {
            let maker = match makers.iter().position(|m| m.name == format.maker) {
                Some(m) => &mut makers[m],
                None => {
                    makers.push(Self { name: format.maker, records: Vec::new() });
                    makers.last_mut().expect("just pushed")
                }
            };
            let record = match maker.records.iter().position(|r| r.name == format.record) {
                Some(r) => &mut maker.records[r],
                None => {
                    maker.records.push(TimecodeRecordDto { name: format.record, sides: Vec::new() });
                    maker.records.last_mut().expect("just pushed")
                }
            };
            record.sides.push(format.into());
        }
        makers
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "TimecodeFormat"))]
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

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "VinylMode"))]
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
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "VinylChange"))]
pub enum VinylChangeDto {
    Mode(VinylModeDto),
    Rpm(RpmDto),
    LeadIn(Option<f64>),
}

/// The speed the record turns at when the track plays at its own speed.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "Rpm"))]
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

/// The timecode as a deck hears it: none, a signal that comes and goes, a good one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "SignalState"))]
pub enum SignalDto {
    None,
    Bounce,
    Ok,
}

impl From<SignalState> for SignalDto {
    fn from(signal: SignalState) -> Self {
        match signal {
            SignalState::None => SignalDto::None,
            SignalState::Bounce => SignalDto::Bounce,
            SignalState::Ok => SignalDto::Ok,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "VinylStatus"))]
pub struct VinylDto {
    pub enabled: bool,
    pub mode: VinylModeDto,
    /// The needle reached the end of the record: the deck plays on by itself.
    pub run_out: bool,
    /// The record has read backwards at play speed for a while: crossed channels?
    pub crossed: bool,
    pub signal: SignalDto,
    /// The signal as the deck's light shows it: none while no record drives it, none heard while
    /// its channels seem crossed.
    pub light: Option<SignalDto>,
    /// Timecode carrier level, as a share of a healthy line level's (0 to 1).
    pub level: f32,
    pub record_speed: f64,
    pub record_position_s: Option<f64>,
}
