//! What the actions keep between two presses, beside the engine and the settings: a deck's
//! cue played while held, its slicer and the slices and cuts held, its roll, its effect
//! focused, its filter set aside, its tempo tapped, its search, when its level last clipped.

use crate::input::Faders;
use crate::press::Held;
use crate::tap::TapTempo;
use prisme_control::MAX_DECKS;
use prisme_engine::EngineStatus;

/// How long a light shows a level past full scale after it.
const CLIP_SHOWN_S: f64 = 1.0;

#[derive(Debug, Default)]
pub struct PerformState {
    pub decks: [DeckPerform; MAX_DECKS],
    pub faders: Faders,
    main_clip: ClipHold,
}

#[derive(Debug, Default)]
pub struct DeckPerform {
    /// Its cue held on the cue point: the track plays until the cue is let go.
    pub cue_preview: bool,
    /// Where the bar its slices divide starts (s).
    pub slicer: Option<f64>,
    pub slices: Held,
    pub cuts: Held,
    /// The roll playing, in beats.
    pub roll: Option<f64>,
    /// The slot of its effect chain the effect knobs turn.
    pub fx_focus: Option<usize>,
    /// Where its filter stood when set aside.
    pub filter_aside: Option<f32>,
    pub taps: TapTempo,
    /// The way it runs through its track while a search key is held: -1 back, 1 forward.
    pub search: Option<i8>,
    clip: ClipHold,
}

/// When a level last went past full scale.
#[derive(Debug, Clone, Copy, Default)]
struct ClipHold {
    at_s: Option<f64>,
}

impl ClipHold {
    /// A level of `peak` heard at `now_s` (seconds, on any clock that goes on): whether it
    /// shows clipping.
    fn heard(&mut self, peak: f32, now_s: f64) -> bool {
        if peak >= 1.0 {
            self.at_s = Some(now_s);
        }
        self.at_s.is_some_and(|at| now_s - at < CLIP_SHOWN_S)
    }
}

/// What the lights read of the performance state.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PerformView {
    pub decks: [DeckPerformView; MAX_DECKS],
    pub faders: Faders,
    pub main_clipping: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DeckPerformView {
    /// Its level went past full scale within the last second.
    pub clipping: bool,
    pub roll: Option<f64>,
    /// The slice playing, of those held.
    pub slice: Option<usize>,
    /// The fader cut playing, of those held.
    pub cut: Option<usize>,
    pub fx_focus: Option<usize>,
}

impl PerformState {
    /// What the lights read of it at `now_s`, the engine's levels at `status` heard first.
    pub fn view(&mut self, status: &EngineStatus, now_s: f64) -> PerformView {
        let decks = std::array::from_fn(|d| {
            let deck = &mut self.decks[d];
            DeckPerformView {
                clipping: deck.clip.heard(status.decks[d].peak, now_s),
                roll: deck.roll,
                slice: deck.slices.acting(),
                cut: deck.cuts.acting(),
                fx_focus: deck.fx_focus,
            }
        });
        PerformView {
            decks,
            faders: self.faders,
            main_clipping: self.main_clip.heard(status.main_peak, now_s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_level_past_full_scale_shows_for_a_second() {
        let mut state = PerformState::default();
        let mut status = EngineStatus::default();
        status.decks[1].peak = 1.2;
        let view = state.view(&status, 10.0);
        assert!(view.decks[1].clipping);
        assert!(!view.decks[0].clipping && !view.main_clipping);
        status.decks[1].peak = 0.5;
        assert!(state.view(&status, 10.9).decks[1].clipping, "held");
        assert!(!state.view(&status, 11.0).decks[1].clipping);
        status.main_peak = 1.0;
        assert!(state.view(&status, 12.0).main_clipping);
    }

    #[test]
    fn the_slice_and_the_cut_playing_are_the_last_pressed_of_those_held() {
        let mut state = PerformState::default();
        state.decks[0].slices.press(2, true);
        state.decks[0].slices.press(5, true);
        state.decks[1].cuts.press(1, true);
        let view = state.view(&EngineStatus::default(), 0.0);
        assert_eq!((view.decks[0].slice, view.decks[1].cut, view.decks[1].slice), (Some(5), Some(1), None));
    }
}
