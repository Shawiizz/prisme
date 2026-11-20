//! What the application's actions decide, apart from the engine, the files and the windows
//! they drive: the rules of each action, and what its light and its fader show of the
//! application at a moment (its [`view::View`]).

pub mod cue;
pub mod hold;
pub mod input;
pub mod knobs;
pub mod lane;
pub mod light;
pub mod meter;
pub mod pads;
pub mod perform;
pub mod player;
pub mod position;
pub mod press;
pub mod scales;
pub mod screen;
pub mod stems;
pub mod switch;
pub mod sync;
pub mod tap;
pub mod view;

use prisme_control::{Band, Effect, Side};
use prisme_engine::EffectKind;

/// The engine's band of `band`.
pub fn engine_band(band: Band) -> prisme_engine::Band {
    match band {
        Band::Low => prisme_engine::Band::Low,
        Band::Mid => prisme_engine::Band::Mid,
        Band::High => prisme_engine::Band::High,
    }
}

/// The engine's side of the crossfader for `side`.
pub fn engine_side(side: Side) -> prisme_engine::CrossfaderSide {
    match side {
        Side::Left => prisme_engine::CrossfaderSide::Left,
        Side::Center => prisme_engine::CrossfaderSide::Center,
        Side::Right => prisme_engine::CrossfaderSide::Right,
    }
}

/// The engine's effect that plays `effect`.
pub fn engine_effect(effect: Effect) -> EffectKind {
    match effect {
        Effect::Echo => EffectKind::Echo,
        Effect::PingPong => EffectKind::PingPong,
        Effect::Reverb => EffectKind::Reverb,
        Effect::Flanger => EffectKind::Flanger,
        Effect::Phaser => EffectKind::Phaser,
        Effect::Filter => EffectKind::Filter,
        Effect::Gate => EffectKind::Gate,
        Effect::Bitcrusher => EffectKind::Bitcrusher,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_actions_name_as_many_decks_and_slots_as_the_engine_has() {
        assert_eq!(prisme_control::SAMPLE_SLOTS, prisme_engine::SAMPLE_SLOTS);
        assert_eq!(prisme_control::FX_SLOTS, prisme_engine::FX_SLOTS);
        assert_eq!(prisme_control::MAX_DECKS, prisme_engine::MAX_DECKS);
    }
}
