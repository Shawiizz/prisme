//! What the application's actions decide, apart from the engine, the files and the windows
//! they drive: the rules of each action, and what its light and its fader show of the
//! application at a moment (its [`view::View`]).

pub mod cue;
pub mod input;
pub mod key;
pub mod lane;
pub mod light;
pub mod pads;
pub mod perform;
pub mod position;
pub mod press;
pub mod scales;
pub mod screen;
pub mod stems;
pub mod switch;
pub mod sync;
pub mod tap;
pub mod view;

#[cfg(test)]
mod tests {
    #[test]
    fn the_actions_name_as_many_decks_and_slots_as_the_engine_has() {
        assert_eq!(prisme_control::SAMPLE_SLOTS, prisme_engine::SAMPLE_SLOTS);
        assert_eq!(prisme_control::FX_SLOTS, prisme_engine::FX_SLOTS);
        assert_eq!(prisme_control::MAX_DECKS, prisme_engine::MAX_DECKS);
    }
}
