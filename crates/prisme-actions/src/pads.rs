//! What a deck's performance pads play: the lengths of its loops, rolls and fader cuts, and its
//! slicer, the eight beats from the start of a bar, each a slice a pad plays round and round
//! while held, the track going on underneath; the pages of pads the screen shows, and what
//! they light.

use crate::light::{light, State};
use crate::view::View;
use prisme_control::{Beats, DeckAction, Effect, FxAction, Target, CUTS, FX_SLOTS, SLICES};
pub use prisme_control::{CUT_BEATS, ROLL_BEATS};
use prisme_engine::LoopStatus;
use serde::Serialize;

/// The pads of a page of the screen.
pub const PADS: usize = 8;
/// The loops of the screen's LOOP pads, in beats.
pub const LOOP_BEATS: [f64; PADS] = [0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0];
/// The jumps of its JUMP pads, in beats.
const JUMP_BEATS: [f64; PADS] = [-32.0, -16.0, -4.0, -1.0, 1.0, 4.0, 16.0, 32.0];

/// A loop this close to a length (of a beat, or of the deck's size) has it.
const LOOP_LENGTH_WITHIN: f64 = 0.01;

/// Whether a deck plays (`looping`) a loop of `beats`, on its grid: the key of that loop
/// leaves it, its light is lit.
pub fn plays_loop(looping: &LoopStatus, beats: f64) -> bool {
    looping.active && looping.beats.is_some_and(|playing| (playing - beats).abs() < LOOP_LENGTH_WITHIN)
}

/// A page of a deck's pads on the screen whose pads each perform an action of the deck.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "PadPage"))]
pub enum Page {
    Loops,
    Roll,
    Jump,
    Slice,
    Cut,
    /// The effects, on the slot of the chain the screen acts on.
    Fx,
}

impl Page {
    pub const ALL: [Page; 6] = [Page::Loops, Page::Roll, Page::Jump, Page::Slice, Page::Cut, Page::Fx];

    /// How many pads it has.
    pub fn pads(self) -> usize {
        match self {
            Page::Loops => LOOP_BEATS.len(),
            Page::Roll => ROLL_BEATS.len(),
            Page::Jump => JUMP_BEATS.len(),
            Page::Slice => SLICES,
            Page::Cut => CUTS,
            Page::Fx => Effect::NAMED.len(),
        }
    }

    /// The action of its pad `pad`, the effects' on the slot `fx_shown`; none past its last.
    pub fn action(self, pad: usize, fx_shown: usize) -> Option<DeckAction> {
        (pad < self.pads()).then(|| match self {
            Page::Loops => DeckAction::LoopBeats(Beats(LOOP_BEATS[pad])),
            Page::Roll => DeckAction::Roll(pad),
            Page::Jump => DeckAction::JumpBeats(Beats(JUMP_BEATS[pad])),
            Page::Slice => DeckAction::Slice(pad),
            Page::Cut => DeckAction::Cut(pad),
            Page::Fx => DeckAction::Fx(fx_shown, FxAction::Effect(Effect::NAMED[pad])),
        })
    }

    /// The action of each of its pads, the effects' on the slot `fx_shown`.
    pub fn actions(self, fx_shown: usize) -> Vec<DeckAction> {
        (0..self.pads()).filter_map(|pad| self.action(pad, fx_shown)).collect()
    }
}

/// What the pads of a page show, a bit per pad: lit by their action's light, and marked (a
/// loop or a jump of the deck's size, an effect sounding in another slot of the chain).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PageLights {
    pub lit: u8,
    pub marked: u8,
}

/// What the pads of each page of a deck show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "PadsLights"))]
pub struct Pads {
    pub loops: PageLights,
    pub roll: PageLights,
    pub jump: PageLights,
    pub slice: PageLights,
    pub cut: PageLights,
    pub fx: PageLights,
}

impl Pads {
    /// Those of `deck` as `view` shows them.
    pub fn of(view: &View, deck: usize) -> Self {
        let page = |page| page_lights(view, deck, page);
        Pads {
            loops: page(Page::Loops),
            roll: page(Page::Roll),
            jump: page(Page::Jump),
            slice: page(Page::Slice),
            cut: page(Page::Cut),
            fx: page(Page::Fx),
        }
    }
}

fn page_lights(view: &View, deck: usize, page: Page) -> PageLights {
    let settings = &view.decks[deck];
    let fx_shown = view.perform.decks[deck].fx_shown;
    let bits = |pad: &dyn Fn(usize, DeckAction) -> bool| {
        let actions = (0..page.pads()).filter_map(|i| page.action(i, fx_shown));
        actions.enumerate().fold(0, |bits, (i, action)| bits | u8::from(pad(i, action)) << i)
    };
    let sounds = |slot: usize, effect: Effect| {
        let fx = &settings.fx.slots[slot];
        fx.enabled && fx.effect == effect
    };
    let lit =
        bits(&|_, action| light(&Target::Deck(deck, action), view).is_some_and(|l| l.state != State::Off));
    let marked = match page {
        Page::Loops => bits(&|i, _| LOOP_BEATS[i] == settings.size_beats),
        Page::Jump => bits(&|i, _| JUMP_BEATS[i].abs() == settings.size_beats),
        Page::Fx => {
            bits(&|i, _| (0..FX_SLOTS).any(|slot| slot != fx_shown && sounds(slot, Effect::NAMED[i])))
        }
        Page::Roll | Page::Slice | Page::Cut => 0,
    };
    PageLights { lit, marked }
}

/// The slice of the slicer starting at `start_s` a track at `position_s` is in, if it is in it,
/// a beat lasting `beat_s`.
pub fn slice_at(start_s: f64, position_s: f64, beat_s: f64) -> Option<usize> {
    let slice = ((position_s - start_s) / beat_s).floor();
    (slice >= 0.0 && slice < SLICES as f64).then_some(slice as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::fixtures::{loaded, view};

    #[test]
    fn each_pad_of_a_page_has_its_action_and_none_past_the_last() {
        for page in Page::ALL {
            assert_eq!(page.actions(1).len(), page.pads(), "{page:?}");
            assert_eq!(page.action(page.pads(), 1), None, "{page:?}");
        }
        assert_eq!(Page::Fx.action(0, 2), Some(DeckAction::Fx(2, FxAction::Effect(Effect::NAMED[0]))));
        assert_eq!(Page::Cut.pads(), CUTS);
    }

    #[test]
    fn the_slicer_starts_on_the_bar_the_track_is_in_and_holds_eight_beats() {
        // 120 BPM from 0.1 s: a bar of 2 s
        let grid = prisme_engine::BeatGrid { bpm: 120.0, first_beat_s: 0.1, bars_sure: true };
        assert!((grid.bar_start_s(5.3) - 4.1).abs() < 1e-9);
        assert!((grid.bar_start_s(0.05) + 1.9).abs() < 1e-9, "before the first beat");
        assert_eq!(slice_at(4.1, 4.1, 0.5), Some(0));
        assert_eq!(slice_at(4.1, 7.0, 0.5), Some(5));
        assert_eq!(slice_at(4.1, 8.1, 0.5), None, "past its eight beats");
        assert_eq!(slice_at(4.1, 4.0, 0.5), None);
    }

    #[test]
    fn a_loop_has_a_length_within_a_hundredth_of_a_beat_on_a_grid() {
        let looping = |beats: Option<f64>| LoopStatus { beats, active: true, ..LoopStatus::default() };
        assert!(plays_loop(&looping(Some(4.0)), 4.0));
        assert!(plays_loop(&looping(Some(8.005)), 8.0));
        assert!(!plays_loop(&looping(Some(8.05)), 8.0), "within 1 %, not within a hundredth");
        assert!(!plays_loop(&looping(Some(4.0)), 2.0));
        assert!(!plays_loop(&looping(None), 4.0), "no grid");
        let stored = LoopStatus { active: false, ..looping(Some(4.0)) };
        assert!(!plays_loop(&stored, 4.0), "stored, not playing");
    }

    #[test]
    fn the_pads_light_the_loop_roll_slice_and_cut_playing_and_mark_the_size() {
        let mut v = loaded(view(), 0, 5.2, true);
        v.status.decks[0].looping = LoopStatus { beats: Some(2.0), active: true, ..LoopStatus::default() };
        v.decks[0].size_beats = 4.0;
        v.perform.decks[0].roll = Some(0.25);
        v.perform.decks[0].cut = Some(1);
        v.perform.decks[0].slicer = Some(4.0);
        let pads = Pads::of(&v, 0);
        assert_eq!(pads.loops, PageLights { lit: 1 << 3, marked: 1 << 4 });
        assert_eq!(pads.jump, PageLights { lit: 0, marked: 1 << 2 | 1 << 5 });
        assert_eq!(pads.roll.lit, 1 << 2);
        assert_eq!(pads.cut.lit, 1 << 1);
        assert_eq!(pads.slice.lit, 1 << 2, "120 BPM: the third half second from 4 s");
        let other = Pads::of(&v, 1);
        let lit =
            [other.loops, other.roll, other.jump, other.slice, other.cut, other.fx].map(|page| page.lit);
        assert_eq!(lit, [0; 6], "no track, nothing played");
    }

    #[test]
    fn the_effect_pads_light_the_slot_shown_and_mark_those_sounding_elsewhere() {
        let mut v = view();
        v.perform.decks[0].fx_shown = 1;
        v.decks[0].fx.slots[0].enabled = true;
        v.decks[0].fx.slots[0].effect = Effect::Reverb;
        v.decks[0].fx.slots[1].enabled = true;
        v.decks[0].fx.slots[1].effect = Effect::Echo;
        assert_eq!(Pads::of(&v, 0).fx, PageLights { lit: 1, marked: 1 << 2 });
    }
}
