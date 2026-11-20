//! Where the fader of an action stands, read from what the actions see ([`Sees`]), and what a
//! value a profile shows on a controller is: a fader's position, or a deck's track measured.

use crate::scales::{balance_knob, eq_knob, filter_knob, headphone_knob, live_knob, trim_knob, Tempo};
use crate::view::{band_index, Sees, View};
use prisme_control::{DeckAction, Effect, FxAction, Kind, Measure, MixerAction, Source, Target, FX_SLOTS};

/// The periods an effect's period fader goes through, in beats.
pub const FX_PERIODS: [f32; 7] = [0.125, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0];
/// Turns a minute of the record a jog wheel's ring shows.
const RECORD_RPM: f64 = 100.0 / 3.0;

/// The fader of a deck's effect chain knob `knob` turns, `focus` the slot focused, as Mixxx's
/// effect unit has them: with none, the amount of effect `knob`; with one, its amount, second
/// knob and period.
pub fn fx_knob(knob: usize, focus: Option<usize>) -> DeckAction {
    match focus {
        None => DeckAction::Fx(knob, FxAction::Amount),
        Some(slot) => DeckAction::Fx(slot, [FxAction::Amount, FxAction::Param2, FxAction::Period][knob]),
    }
}

/// Where the fader of `target` stands, 0 to 1, if it has one and its deck is in use: only what
/// it stands by is read of `seen`.
pub fn position(target: &Target, seen: &impl Sees) -> Option<f32> {
    if target.kind() != Kind::Fader {
        return None;
    }
    match target {
        Target::Deck(deck, action) if *deck < seen.decks_in_use() => deck_position(*deck, *action, seen),
        Target::Mixer(action) => mixer_position(*action, seen),
        _ => None,
    }
}

/// The value of `source` as `view` shows it: a fader's position, or a deck's track measured
/// (none without a track, or on a deck not in use).
pub fn output(source: &Source, view: &View) -> Option<f64> {
    let (deck, measure) = match source {
        Source::Position(target) => return position(target, view).map(f64::from),
        Source::Measured(deck, measure) => (*deck, *measure),
    };
    let status = view.deck_status(deck);
    if !view.shows(deck) || !status.loaded {
        return None;
    }
    let (at, length) = (status.position_s, status.duration_s);
    Some(match measure {
        Measure::Position if length > 0.0 => at / length,
        Measure::Position => 0.0,
        Measure::Turn => (at * RECORD_RPM / 60.0).rem_euclid(1.0),
        Measure::Bpm => status.bpm?,
        Measure::Elapsed => at,
        Measure::Remaining => (length - at).max(0.0),
        Measure::Duration => length,
        Measure::Tempo => (view.tempo_rate(deck) - 1.0) * 100.0,
        Measure::TempoRange => view.decks[deck].tempo_range * 100.0,
    })
}

fn deck_position(deck: usize, action: DeckAction, seen: &impl Sees) -> Option<f32> {
    let settings = seen.deck(deck);
    let slots = &settings.fx.slots;
    let period = |slot: usize| {
        let target = Target::Deck(deck, DeckAction::Fx(slot, FxAction::Period));
        let step = FX_PERIODS.iter().position(|p| *p == slots[slot].beats).unwrap_or(0);
        seen.faders().stepped(&target).position(step, FX_PERIODS.len())
    };
    Some(match action {
        DeckAction::Pitch => Tempo { ratio: settings.pitch, range: settings.tempo_range }.position(),
        DeckAction::Volume => settings.volume,
        DeckAction::Trim => trim_knob(settings.strip.trim),
        DeckAction::Eq(band) => {
            let target = Target::Deck(deck, action);
            seen.faders().silent_end(&target).position(eq_knob(settings.strip.eq[band_index(band)]))
        }
        DeckAction::Filter => filter_knob(settings.strip.filter),
        DeckAction::Position => {
            let status = seen.status(deck);
            if status.duration_s > 0.0 {
                (status.position_s / status.duration_s) as f32
            } else {
                0.0
            }
        }
        DeckAction::Key => (f32::from(seen.track(deck).map_or(0, |t| t.key_shift)) + 12.0) / 24.0,
        DeckAction::SamplerGain => settings.sampler_gain,
        DeckAction::StemLevel(part) => seen.track(deck).map_or(1.0, |t| t.stem_mix.level(part)),
        DeckAction::FxSuper => slots.iter().map(|s| s.amount).sum::<f32>() / FX_SLOTS as f32,
        DeckAction::FxKnob(knob) => return deck_position(deck, fx_knob(knob, seen.fx_focus(deck)), seen),
        DeckAction::Fx(slot, FxAction::Mix) => slots[slot].mix,
        DeckAction::Fx(slot, FxAction::Amount) => slots[slot].amount,
        DeckAction::Fx(slot, FxAction::Param2) => slots[slot].param2,
        DeckAction::Fx(slot, FxAction::Period) => period(slot),
        DeckAction::Fx(slot, FxAction::Kind) => {
            let step = Effect::NAMED.iter().position(|e| *e == slots[slot].effect).unwrap_or(0);
            seen.faders().stepped(&Target::Deck(deck, action)).position(step, Effect::NAMED.len())
        }
        _ => return None,
    })
}

fn mixer_position(action: MixerAction, seen: &impl Sees) -> Option<f32> {
    let target = Target::Mixer(action);
    if action == MixerAction::SamplerGain {
        // it sets every deck in use: it stands where they do on the whole
        let decks = seen.decks_in_use();
        let gains = (0..decks).map(|d| seen.deck(d).sampler_gain);
        return Some(gains.sum::<f32>() / decks.max(1) as f32);
    }
    let mixer = seen.mixer();
    Some(match action {
        MixerAction::Crossfader => (mixer.crossfader + 1.0) / 2.0,
        MixerAction::CueMix => mixer.cue_mix,
        MixerAction::Balance => balance_knob(mixer.balance),
        MixerAction::Booth => mixer.booth_gain,
        MixerAction::Mic => seen.faders().silent_end(&target).position(live_knob(mixer.mic.level)),
        MixerAction::Aux => seen.faders().silent_end(&target).position(live_knob(mixer.aux.level)),
        MixerAction::Headphones => headphone_knob(mixer.headphone_gain),
        MixerAction::Main => mixer.main_gain,
        MixerAction::CrossfaderCurve => {
            seen.faders().stepped(&target).position(usize::from(mixer.sharp_curve), 2)
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::fixtures::{loaded, view};
    use prisme_control::StemPart;

    fn at(id: &str, view: &View) -> Option<f32> {
        position(&id.parse().unwrap(), view)
    }

    fn shown(id: &str, view: &View) -> Option<f64> {
        output(&id.parse().unwrap(), view)
    }

    fn close(a: Option<f32>, b: f32) -> bool {
        a.is_some_and(|a| (a - b).abs() < 1e-5)
    }

    #[test]
    fn the_tempo_fader_stands_where_the_tempo_is_in_its_range() {
        let mut v = view();
        assert_eq!(at("deck1.pitch", &v), Some(0.5));
        v.decks[0].pitch = 1.04;
        assert!(close(at("deck1.pitch", &v), 0.75));
        v.decks[0].pitch = 0.5;
        assert_eq!(at("deck1.pitch", &v), Some(0.0));
    }

    #[test]
    fn the_channel_and_the_mixer_stand_on_their_scales() {
        let mut v = view();
        v.decks[1].volume = 0.7;
        v.decks[1].strip.filter = -1.0;
        v.decks[1].strip.eq[0] = 0.0;
        v.mixer.crossfader = 1.0;
        v.mixer.headphone_gain = 1.5;
        v.mixer.sharp_curve = true;
        assert_eq!(at("deck2.volume", &v), Some(0.7));
        assert_eq!(at("deck2.filter", &v), Some(0.0));
        assert_eq!(at("deck2.eq.low", &v), Some(0.0), "silent");
        assert!(close(at("deck2.eq.mid", &v), 0.5));
        assert!(close(at("deck2.trim", &v), 0.5));
        assert_eq!(at("mixer.crossfader", &v), Some(1.0));
        assert_eq!(at("mixer.headphones", &v), Some(1.0));
        assert_eq!(at("mixer.crossfaderCurve", &v), Some(0.75));
        assert!(close(at("mixer.mic", &v), 0.75), "0 dB");
        assert_eq!(at("deck2.play", &v), None);
    }

    #[test]
    fn a_fader_of_a_deck_not_in_use_stands_nowhere() {
        assert_eq!(at("deck3.volume", &view()), None);
    }

    #[test]
    fn the_samplers_level_stands_where_the_decks_in_use_are_on_the_whole() {
        let mut v = view();
        v.decks[0].sampler_gain = 0.2;
        v.decks[1].sampler_gain = 0.6;
        v.decks[2].sampler_gain = 0.0;
        assert!(close(at("sampler.gain", &v), 0.4));
        assert_eq!(at("deck1.sampler.gain", &v), Some(0.2));
    }

    #[test]
    fn the_three_effect_knobs_stand_on_the_amounts_or_on_the_focused_effect() {
        let mut v = view();
        for (slot, amount) in [0.1, 0.2, 0.3].into_iter().enumerate() {
            v.decks[0].fx.slots[slot].amount = amount;
        }
        let knobs = |v: &View| [1, 2, 3].map(|k| at(&format!("deck1.fx.knob{k}"), v).unwrap());
        assert_eq!(knobs(&v), [0.1, 0.2, 0.3]);
        assert!(close(at("deck1.fx.super", &v), 0.2));
        v.decks[0].fx.slots[1].param2 = 0.8;
        v.decks[0].fx.slots[1].beats = 0.125;
        v.perform.decks[0].fx_focus = Some(1);
        assert_eq!(knobs(&v), [0.2, 0.8, 0.5 / 7.0]);
    }

    #[test]
    fn the_period_and_the_effect_faders_stand_in_the_middle_of_their_step() {
        let mut v = view();
        assert!(close(at("deck1.fx3.period", &v), 3.5 / 7.0));
        v.decks[0].fx.slots[2].beats = 2.0;
        assert!(close(at("deck1.fx3.period", &v), 4.5 / 7.0));
        v.decks[0].fx.slots[2].effect = Effect::Flanger;
        assert!(close(at("deck1.fx3.kind", &v), 3.5 / 8.0));
    }

    #[test]
    fn a_fader_over_steps_or_silent_at_an_end_stands_where_it_was_given_while_it_gives_the_setting() {
        let mut v = view();
        let target = |id: &str| id.parse::<Target>().unwrap();
        // a period of 1/4 beat given at 0.2, the knob of the focused slot reading it too
        v.decks[0].fx.slots[1].beats = 0.25;
        v.faders.give_stepped(&target("deck1.fx2.period"), 0.2, 7);
        v.perform.decks[0].fx_focus = Some(1);
        assert_eq!(at("deck1.fx2.period", &v), Some(0.2));
        assert_eq!(at("deck1.fx.knob3", &v), Some(0.2));
        v.decks[0].fx.slots[1].beats = 2.0;
        assert!(close(at("deck1.fx2.period", &v), 4.5 / 7.0), "changed elsewhere");
        // a band silent at 0.01, a live source at 0.015
        v.decks[1].strip.eq[2] = 0.0;
        v.faders.give_silent_end(&target("deck2.eq.high"), 0.01, 0.0);
        assert_eq!(at("deck2.eq.high", &v), Some(0.01));
        assert_eq!(at("deck1.eq.high", &v), Some(eq_knob(1.0)), "the other deck's");
        v.mixer.aux.level = 0.0;
        v.faders.give_silent_end(&target("mixer.aux"), 0.015, live_knob(0.0));
        assert_eq!(at("mixer.aux", &v), Some(0.015));
        v.faders.give_stepped(&target("mixer.crossfaderCurve"), 0.9, 2);
        v.mixer.sharp_curve = true;
        assert_eq!(at("mixer.crossfaderCurve", &v), Some(0.9));
    }

    #[test]
    fn the_key_and_the_stems_stand_where_the_track_has_them() {
        let mut v = view();
        assert_eq!(at("deck1.key", &v), Some(0.5));
        assert_eq!(at("deck1.vocals.level", &v), Some(1.0), "no track");
        v = loaded(v, 0, 0.0, false);
        if let Some(track) = &mut v.decks[0].track {
            track.key_shift = 6;
            track.stem_mix = track.stem_mix.with_level(StemPart::Vocals, 0.25);
        }
        assert_eq!(at("deck1.key", &v), Some(0.75));
        assert_eq!(at("deck1.vocals.level", &v), Some(0.25));
    }

    #[test]
    fn a_deck_shows_its_place_its_record_its_tempo_and_its_times() {
        let mut v = loaded(view(), 0, 75.0, true);
        assert_eq!(shown("deck1.position", &v), Some(0.25));
        assert_eq!(at("deck1.position", &v), Some(0.25));
        // 75 s at 33⅓ turns a minute: 41⅔ turns
        assert!((shown("deck1.turn", &v).unwrap() - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(shown("deck1.bpm", &v), Some(120.0));
        assert_eq!(shown("deck1.elapsed", &v), Some(75.0));
        assert_eq!(shown("deck1.remaining", &v), Some(225.0));
        assert_eq!(shown("deck1.duration", &v), Some(300.0));
        v.status.decks[0].rate = 1.05;
        assert!((shown("deck1.tempo", &v).unwrap() - 5.0).abs() < 1e-9);
        assert!((shown("deck1.tempoRange", &v).unwrap() - 8.0).abs() < 1e-9);
        assert_eq!(shown("deck2.turn", &v), None, "no track");
        assert_eq!(shown("deck1.volume", &v), Some(1.0));
        assert_eq!(shown("deck3.duration", &loaded(view(), 2, 0.0, false)), None, "not in use");
    }
}
