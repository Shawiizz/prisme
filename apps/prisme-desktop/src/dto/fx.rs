//! The decks' effect chains.

use prisme_engine::FX_SLOTS;
use serde::{Deserialize, Serialize};

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
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "StemPart"))]
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
