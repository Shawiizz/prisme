//! The decks' effect chains.

use prisme_control::{Effect, StemPart};
use prisme_engine::FX_SLOTS;
use serde::Serialize;

/// A slot of a deck's effect chain.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "FxSettings"))]
pub struct FxDto {
    pub kind: Effect,
    pub enabled: bool,
    /// Level of the effect, 0..1.
    pub mix: f32,
    /// The effect's main knob, 0..1.
    pub amount: f32,
    /// The effect's second knob (tone, resonance...), 0..1; in the middle, the effect sounds as
    /// it did without one.
    pub param2: f32,
    /// The effect's period in beats.
    pub beats: f32,
}

impl Default for FxDto {
    fn default() -> Self {
        Self {
            kind: Effect::Echo,
            enabled: false,
            mix: 0.5,
            amount: 0.5,
            param2: prisme_engine::PARAM2_DEFAULT,
            beats: 1.0,
        }
    }
}

/// A deck's beat effects, in series.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "FxChain"))]
pub struct FxChainDto {
    pub slots: [FxDto; FX_SLOTS],
    /// The whole chain on, each slot then as its own switch says; off, all silent.
    pub enabled: bool,
    /// The part of the track the chain is on, where separated; none: the whole deck.
    pub part: Option<StemPart>,
}

impl Default for FxChainDto {
    fn default() -> Self {
        Self { slots: Default::default(), enabled: true, part: None }
    }
}
