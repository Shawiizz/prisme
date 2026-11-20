//! The knobs of the screen's mixer: where each stands, as a controller's fader would (see
//! [`position`]), what it reads, and where a double click puts it back.

use crate::position::position;
use crate::scales::{headphone_knob, live_knob, Readout};
use crate::view::{band_index, View};
use prisme_control::{Band, DeckAction, MixerAction, Target};
use serde::Serialize;

/// A knob of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Knob {
    /// Where it stands, from 0 to 1.
    pub at: f32,
    pub reads: Readout,
    /// Where a double click puts it back: unity gain, or the centre.
    pub home: f32,
}

/// The knobs of a deck's channel: trim, EQ (low, mid, high) and filter.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct StripKnobs {
    pub trim: Knob,
    pub eq: [Knob; 3],
    pub filter: Knob,
}

/// A fader of the screen that reads nothing under it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Fader {
    /// Where it stands, from 0 to 1.
    pub at: f32,
    /// Where a double click puts it back.
    pub home: f32,
}

/// The knobs of the mixer beside its channels, and its crossfader.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MixerKnobs {
    /// The crossfader, from 0 (left) to 1, back in the middle.
    pub crossfader: Fader,
    pub cue_mix: Knob,
    pub headphones: Knob,
    pub main: Knob,
    pub balance: Knob,
    pub booth: Knob,
    pub mic: Knob,
    pub aux: Knob,
}

const CENTRE: f32 = 0.5;

fn at(target: Target, view: &View) -> f32 {
    position(&target, view).unwrap_or(CENTRE)
}

/// The knobs of `deck`'s channel in `view`.
pub fn strip(view: &View, deck: usize) -> StripKnobs {
    let strip = view.decks[deck].strip;
    let knob = |action, reads| Knob { at: at(Target::Deck(deck, action), view), reads, home: CENTRE };
    StripKnobs {
        trim: knob(DeckAction::Trim, Readout::db(strip.trim)),
        eq: [Band::Low, Band::Mid, Band::High]
            .map(|band| knob(DeckAction::Eq(band), Readout::db(strip.eq[band_index(band)]))),
        filter: knob(DeckAction::Filter, Readout::filter(strip.filter)),
    }
}

/// The knobs of the mixer in `view`.
pub fn mixer(view: &View) -> MixerKnobs {
    let mixer = &view.mixer;
    let knob = |action, reads, home| Knob { at: at(Target::Mixer(action), view), reads, home };
    MixerKnobs {
        crossfader: Fader { at: at(Target::Mixer(MixerAction::Crossfader), view), home: CENTRE },
        cue_mix: knob(MixerAction::CueMix, Readout::percent(mixer.cue_mix), 0.0),
        headphones: knob(MixerAction::Headphones, Readout::db(mixer.headphone_gain), headphone_knob(1.0)),
        main: knob(MixerAction::Main, Readout::db(mixer.main_gain), 1.0),
        balance: knob(MixerAction::Balance, Readout::balance(mixer.balance), CENTRE),
        booth: knob(MixerAction::Booth, Readout::db(mixer.booth_gain), 1.0),
        mic: knob(MixerAction::Mic, Readout::db(mixer.mic.level), live_knob(1.0)),
        aux: knob(MixerAction::Aux, Readout::db(mixer.aux.level), live_knob(1.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scales::{eq_gain, eq_knob, filter_knob, live_gain};
    use crate::view::fixtures::view;

    #[test]
    fn a_channels_knobs_stand_and_read_on_their_scales() {
        let mut view = view();
        view.decks[0].strip.trim = 1.0;
        view.decks[0].strip.eq[2] = eq_gain(1.0);
        view.decks[0].strip.filter = -0.5;
        let knobs = strip(&view, 0);
        assert_eq!((knobs.trim.at, knobs.trim.reads), (0.5, Readout::Db(Some(0.0))));
        assert_eq!((knobs.eq[2].at, knobs.eq[2].reads), (1.0, Readout::Db(Some(6.0))));
        assert_eq!((knobs.filter.at, knobs.filter.reads), (filter_knob(-0.5), Readout::Filter(-50)));
        assert_eq!(knobs.eq[0].home, 0.5);
    }

    #[test]
    fn an_eq_turned_into_its_silent_end_stands_where_it_was_given() {
        let mut view = view();
        let low = Target::Deck(0, DeckAction::Eq(Band::Low));
        view.decks[0].strip.eq[0] = eq_gain(0.01);
        view.faders.give_silent_end(&low, 0.01, eq_knob(0.0));
        let knob = strip(&view, 0).eq[0];
        assert_eq!((knob.at, knob.reads), (0.01, Readout::Db(None)));
    }

    #[test]
    fn the_mixers_knobs_go_back_to_unity_gain_or_their_centre() {
        let mut view = view();
        view.mixer.crossfader = 1.0;
        view.mixer.headphone_gain = 1.0;
        view.mixer.mic.level = live_gain(1.0);
        let knobs = mixer(&view);
        assert_eq!(knobs.crossfader, Fader { at: 1.0, home: 0.5 });
        assert_eq!(knobs.headphones.home, knobs.headphones.at);
        assert_eq!(knobs.headphones.reads, Readout::Db(Some(0.0)));
        assert_eq!((knobs.mic.at, knobs.mic.reads), (1.0, Readout::Db(Some(12.0))));
        assert_eq!(knobs.mic.home, 0.75);
        assert_eq!((knobs.main.home, knobs.cue_mix.home, knobs.balance.home), (1.0, 0.0, 0.5));
    }
}
