//! What the application's actions decide, apart from the engine, the files and the windows
//! they drive: a CDJ's cue key, the scales of the knobs, the parts of a separated track, the
//! phase and the keys of two decks, how a control's moves reach its action (soft takeover,
//! encoder steps, faders over a few steps or silent at an end), the keys held of a row, the
//! tempo tapped, where an action is performed, what the actions keep between presses, and what
//! their lights and faders show of the application at a moment (its [`view::View`]).

pub mod cue;
pub mod input;
pub mod key;
pub mod lane;
pub mod light;
pub mod perform;
pub mod position;
pub mod press;
pub mod scales;
pub mod stems;
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
