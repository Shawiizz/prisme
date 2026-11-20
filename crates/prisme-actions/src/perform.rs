//! What the actions keep between two presses, beside the engine and the settings: a deck's
//! cue played while held, its slicer and the slices and cuts held, its roll, its effect
//! focused, its filter set aside, its tempo tapped; and when the levels last clipped.

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
}

/// When the decks' levels and the master's last went past full scale.
#[derive(Debug, Default)]
pub struct Clips {
    decks: [ClipHold; MAX_DECKS],
    main: ClipHold,
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

/// Which levels show clipping.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Clipping {
    pub decks: [bool; MAX_DECKS],
    pub main: bool,
}

impl Clips {
    /// The engine's levels at `status` heard at `now_s`: which show clipping.
    pub fn heard(&mut self, status: &EngineStatus, now_s: f64) -> Clipping {
        Clipping {
            decks: std::array::from_fn(|d| self.decks[d].heard(status.decks[d].peak, now_s)),
            main: self.main.heard(status.main_peak, now_s),
        }
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
    pub slicer: Option<f64>,
    /// The fader cut playing, of those held.
    pub cut: Option<usize>,
    pub fx_focus: Option<usize>,
}

impl PerformState {
    /// What the lights read of it, the levels clipping as `clipping` says.
    pub fn view(&self, clipping: Clipping) -> PerformView {
        let decks = std::array::from_fn(|d| {
            let deck = &self.decks[d];
            DeckPerformView {
                clipping: clipping.decks[d],
                roll: deck.roll,
                slicer: deck.slicer,
                cut: deck.cuts.acting(),
                fx_focus: deck.fx_focus,
            }
        });
        PerformView { decks, faders: self.faders, main_clipping: clipping.main }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_level_past_full_scale_shows_for_a_second() {
        let mut clips = Clips::default();
        let mut status = EngineStatus::default();
        status.decks[1].peak = 1.2;
        let clipping = clips.heard(&status, 10.0);
        assert!(clipping.decks[1]);
        assert!(!clipping.decks[0] && !clipping.main);
        status.decks[1].peak = 0.5;
        assert!(clips.heard(&status, 10.9).decks[1], "held");
        assert!(!clips.heard(&status, 11.0).decks[1]);
        status.main_peak = 1.0;
        assert!(clips.heard(&status, 12.0).main);
    }

    #[test]
    fn the_cut_playing_is_the_last_pressed_of_those_held() {
        let mut state = PerformState::default();
        state.decks[1].cuts.press(1, true);
        state.decks[1].cuts.press(3, true);
        state.decks[0].roll = Some(0.5);
        let view = state.view(Clipping::default());
        assert_eq!((view.decks[1].cut, view.decks[0].cut, view.decks[0].roll), (Some(3), None, Some(0.5)));
    }
}
