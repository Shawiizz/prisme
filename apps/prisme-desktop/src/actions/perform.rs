//! What each action does, by its target, with what its control gives: a key pressed or let go,
//! a fader's position, an encoder's steps.

use super::{deck, fx, mixer, not_performed, sampler, transport, Performing};
use crate::dto::{CrossfaderCurveDto, EffectKindDto, LiveSourceDto};
use crate::error::AppResult;
use crate::state::lock;
use prisme_actions::input::{Given, Input};
use prisme_actions::position::FX_PERIODS;
use prisme_actions::scales::{
    balance_amount, eq_gain, eq_knob, filter_amount, headphone_gain, live_gain, live_knob, trim_gain, Tempo,
};
use prisme_control::{
    AutoDjAction, Beats, DeckAction, Effect, FxAction, Kind, MixerAction, SampleAction, Target, FX_SLOTS,
};
use prisme_engine::Command;
use transport::{PITCH_BEND, SMALL_BEND};

impl Performing<'_> {
    /// Performs `target` driven by `input`: nothing on a deck not in use.
    pub(super) fn perform(&self, target: &Target, input: Input) -> AppResult<()> {
        if target.deck().is_some_and(|deck| self.state.has_deck(deck).is_err()) {
            return Ok(());
        }
        if let Target::Deck(deck, action @ (DeckAction::Jog | DeckAction::JogTouch | DeckAction::JogSearch)) =
            *target
        {
            return deck::turn(self, deck, action, input);
        }
        let Some(given) = self.given(target, input) else { return Ok(()) };
        match *target {
            Target::Deck(deck, action) => match given {
                Given::Value(position) if action.kind() == Kind::Fader => {
                    self.deck_fader(deck, action, position)
                }
                _ => self.deck(deck, action, given),
            },
            Target::Mixer(action) => self.mixer(action, given),
            Target::AutoDj(action) => {
                if let Some(auto_dj) = self.auto_dj().filter(|_| given.pressed()) {
                    match action {
                        AutoDjAction::Toggle => auto_dj.toggle(),
                        AutoDjAction::Fade => auto_dj.fade_now(),
                        AutoDjAction::Skip => auto_dj.skip(),
                    }
                }
                Ok(())
            }
            _ => Err(not_performed(target)),
        }
    }

    fn deck(&self, deck: usize, action: DeckAction, given: Given) -> AppResult<()> {
        let state = self.state;
        let held = given.pressed();
        let send = |command| transport::send(state, deck, command);
        let pressed = |command| if held { send(command) } else { Ok(()) };
        let bend =
            |amount: f64| send(Command::SetPitchBend { deck, amount: if held { amount } else { 0.0 } });
        match action {
            DeckAction::SyncLeader if held => deck::toggle_leader(state, deck),
            DeckAction::Brake => pressed(Command::Brake { deck }),
            DeckAction::Spinback => send(Command::SpinBack { deck, held }),
            DeckAction::Censor => send(Command::Censor { deck, active: held }),
            DeckAction::ReverseHeld => send(Command::HoldReverse { deck, held }),
            DeckAction::SlipHeld => send(Command::SetSlip { deck, enabled: held }),
            DeckAction::BendDown => bend(-PITCH_BEND),
            DeckAction::BendUp => bend(PITCH_BEND),
            DeckAction::BendDownSmall => bend(-SMALL_BEND),
            DeckAction::BendUpSmall => bend(SMALL_BEND),
            DeckAction::LoopIn => pressed(Command::LoopIn { deck }),
            DeckAction::LoopOut => pressed(Command::LoopOut { deck }),
            DeckAction::Reloop => pressed(Command::Reloop { deck }),
            DeckAction::LoopHalve => pressed(Command::ScaleLoop { deck, factor: 0.5 }),
            DeckAction::LoopDouble => pressed(Command::ScaleLoop { deck, factor: 2.0 }),
            DeckAction::LoopBeats(Beats(beats)) => pressed(Command::BeatLoop { deck, beats }),
            DeckAction::JumpBeats(Beats(beats)) if held => {
                transport::beat_jump(state, self.auto_dj().as_deref(), deck, beats)
            }
            DeckAction::LoopClear if held => transport::set_loop(state, deck, None, None, false),
            DeckAction::EqKill(band) => mixer::set_eq_kill(state, deck, band.into(), held),
            DeckAction::Fx(slot, FxAction::Held) => fx::set_fx_enabled(state, deck, slot, held),
            DeckAction::Sample(slot, SampleAction::Trigger) if held => {
                sampler::trigger_sample(state, deck, slot, None)
            }
            // as hard as the pad was hit
            DeckAction::Sample(slot, SampleAction::Velocity) => match given {
                Given::Value(velocity) if held => sampler::trigger_sample(state, deck, slot, Some(velocity)),
                _ => Ok(()),
            },
            DeckAction::Sample(slot, SampleAction::Stop) if held => sampler::stop_sample(state, deck, slot),
            DeckAction::Sample(slot, SampleAction::Held) if held => {
                sampler::trigger_sample(state, deck, slot, None)
            }
            DeckAction::Sample(slot, SampleAction::Held) => sampler::stop_sample(state, deck, slot),
            DeckAction::HotCueDelete(slot) if held => self.delete_hot_cue(deck, slot),
            DeckAction::GridHere
            | DeckAction::GridEarlier
            | DeckAction::GridLater
            | DeckAction::GridFaster
            | DeckAction::GridSlower
            | DeckAction::GridMove => self.edit_grid(deck, action, given),
            // a key let go
            DeckAction::SyncLeader
            | DeckAction::JumpBeats(_)
            | DeckAction::LoopClear
            | DeckAction::Sample(_, SampleAction::Trigger | SampleAction::Stop)
            | DeckAction::HotCueDelete(_) => Ok(()),
            _ => Err(not_performed(&Target::Deck(deck, action))),
        }
    }

    /// A fader of `deck` moved to `at`, 0 to 1, on its scale.
    fn deck_fader(&self, deck: usize, action: DeckAction, at: f32) -> AppResult<()> {
        let state = self.state;
        let target = Target::Deck(deck, action);
        match action {
            DeckAction::Pitch => {
                let tempo = Tempo { ratio: lock(&state.decks)[deck].pitch, range: state.tempo_range(deck) };
                deck::set_pitch(state, deck, tempo.at(at))
            }
            DeckAction::Volume => mixer::set_volume(state, deck, at),
            DeckAction::Trim => mixer::set_trim(state, deck, trim_gain(at)),
            DeckAction::Eq(band) => {
                let gain = eq_gain(at);
                mixer::set_eq(state, deck, band.into(), gain)?;
                lock(&self.actions.perform).faders.give_silent_end(&target, at, eq_knob(gain));
                Ok(())
            }
            DeckAction::Filter => mixer::set_filter(state, deck, filter_amount(at)),
            DeckAction::Position => match state.deck_status(deck).map(|s| s.duration_s) {
                Ok(duration_s) if duration_s > 0.0 => {
                    transport::seek(state, self.auto_dj().as_deref(), deck, f64::from(at) * duration_s)
                }
                _ => Ok(()),
            },
            // a track's key, 12 semitones down to 12 up
            DeckAction::Key if state.with_track(deck, |_| ()).is_ok() => {
                deck::set_key_shift(state, deck, (at * 24.0).round() as i8 - 12)
            }
            DeckAction::Key => Ok(()),
            DeckAction::SamplerGain => sampler::set_sampler_gain(state, deck, at),
            DeckAction::StemLevel(part) => deck::set_stem_level(state, deck, part, at),
            DeckAction::FxSuper => {
                (0..FX_SLOTS).try_for_each(|slot| fx::set_fx_amount(state, deck, slot, at))
            }
            DeckAction::Fx(slot, FxAction::Mix) => fx::set_fx_mix(state, deck, slot, at),
            DeckAction::Fx(slot, FxAction::Amount) => fx::set_fx_amount(state, deck, slot, at),
            DeckAction::Fx(slot, FxAction::Param2) => fx::set_fx_param2(state, deck, slot, at),
            DeckAction::Fx(slot, FxAction::Period) => {
                let step = lock(&self.actions.perform).faders.give_stepped(&target, at, FX_PERIODS.len());
                let beats = FX_PERIODS[step];
                if lock(&state.decks)[deck].fx.slots[slot].beats == beats {
                    return Ok(());
                }
                fx::set_fx_beats(state, deck, slot, beats)
            }
            DeckAction::Fx(slot, FxAction::Kind) => {
                let step = lock(&self.actions.perform).faders.give_stepped(&target, at, Effect::NAMED.len());
                let kind = EffectKindDto::from(Effect::NAMED[step]);
                if lock(&state.decks)[deck].fx.slots[slot].kind == kind {
                    return Ok(());
                }
                fx::set_fx(state, deck, slot, kind)
            }
            _ => Err(not_performed(&target)),
        }
    }

    /// An action of the mixer, given `given`.
    fn mixer(&self, action: MixerAction, given: Given) -> AppResult<()> {
        let state = self.state;
        let target = Target::Mixer(action);
        let Given::Value(at) = given else { return Ok(()) };
        let live = |source: LiveSourceDto| {
            let gain = live_gain(at);
            mixer::set_live_level(state, source, gain)?;
            lock(&self.actions.perform).faders.give_silent_end(&target, at, live_knob(gain));
            Ok(())
        };
        match action {
            MixerAction::Crossfader => {
                mixer::set_crossfader(state, self.auto_dj().as_deref(), at * 2.0 - 1.0)
            }
            MixerAction::CueMix => mixer::set_cue_mix(state, at),
            MixerAction::Balance => mixer::set_balance(state, balance_amount(at)),
            MixerAction::Booth => mixer::set_booth_gain(state, at),
            MixerAction::Headphones => mixer::set_headphone_gain(state, headphone_gain(at)),
            MixerAction::Main => mixer::set_main_gain(state, at),
            MixerAction::Mic => live(LiveSourceDto::Mic),
            MixerAction::Aux => live(LiveSourceDto::Aux),
            MixerAction::CrossfaderCurve => {
                let sharp = lock(&self.actions.perform).faders.give_stepped(&target, at, 2) == 1;
                let curve = if sharp { CrossfaderCurveDto::Sharp } else { CrossfaderCurveDto::Smooth };
                if state.preferences.read(|p| p.mixer.crossfader_curve) == curve {
                    return Ok(());
                }
                mixer::set_crossfader_curve(state, curve)
            }
            // every deck in use
            MixerAction::SamplerGain => {
                let decks = state.preferences.read(|p| p.dj.decks());
                (0..decks).try_for_each(|deck| sampler::set_sampler_gain(state, deck, at))
            }
            _ => Err(not_performed(&target)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::actions::bench::Bench;
    use crate::actions::{not_performed, Source};
    use crate::error::AppError;
    use crate::state::lock;
    use prisme_actions::input::Input;
    use prisme_control::{Owner, Target};
    use prisme_engine::{Command, LoopRegion};

    /// What pressing then letting go of `id` sends the engine.
    fn pressed(bench: &mut Bench, id: &str) -> Vec<Command> {
        bench.press(id, 1.0).unwrap();
        bench.press(id, 0.0).unwrap();
        bench.sent()
    }

    #[test]
    fn a_key_sends_its_deck_its_command_once_when_pressed() {
        let mut bench = Bench::new();
        assert!(matches!(pressed(&mut bench, "deck2.brake")[..], [Command::Brake { deck: 1 }]));
        assert!(matches!(pressed(&mut bench, "deck1.loop.in")[..], [Command::LoopIn { deck: 0 }]));
        assert!(matches!(pressed(&mut bench, "deck1.loop.out")[..], [Command::LoopOut { deck: 0 }]));
        assert!(matches!(pressed(&mut bench, "deck2.reloop")[..], [Command::Reloop { deck: 1 }]));
        let halved = pressed(&mut bench, "deck1.loop.halve");
        assert!(matches!(halved[..], [Command::ScaleLoop { deck: 0, factor }] if factor == 0.5));
        let doubled = pressed(&mut bench, "deck1.loop.double");
        assert!(matches!(doubled[..], [Command::ScaleLoop { deck: 0, factor }] if factor == 2.0));
        let looped = pressed(&mut bench, "deck2.loop.0.5");
        assert!(matches!(looped[..], [Command::BeatLoop { deck: 1, beats }] if beats == 0.5));
        let jumped = pressed(&mut bench, "deck1.jump.-8");
        assert!(matches!(jumped[..], [Command::BeatJump { deck: 0, beats }] if beats == -8.0));
        let cleared = pressed(&mut bench, "deck1.loop.clear");
        assert!(matches!(cleared[..], [Command::SetLoop { deck: 0, region: None, active: false }]));
        let soft = 20.0 / 127.0;
        bench.press("deck1.sample3", soft).unwrap();
        bench.press("deck1.sample3.stop", 1.0).unwrap();
        assert!(matches!(
            bench.sent()[..],
            [
                Command::TriggerSample { deck: 0, slot: 2, gain: 1.0 },
                Command::StopSample { deck: 0, slot: 2 }
            ]
        ));
    }

    #[test]
    fn a_held_key_acts_while_held() {
        let mut bench = Bench::new();
        let held = |bench: &mut Bench, id| pressed(bench, id);
        assert!(matches!(
            held(&mut bench, "deck2.spinback")[..],
            [Command::SpinBack { deck: 1, held: true }, Command::SpinBack { deck: 1, held: false }]
        ));
        assert!(matches!(
            held(&mut bench, "deck1.censor")[..],
            [Command::Censor { active: true, .. }, Command::Censor { active: false, .. }]
        ));
        assert!(matches!(
            held(&mut bench, "deck1.reverse.held")[..],
            [Command::HoldReverse { held: true, .. }, Command::HoldReverse { held: false, .. }]
        ));
        assert!(matches!(
            held(&mut bench, "deck1.slip.held")[..],
            [Command::SetSlip { enabled: true, .. }, Command::SetSlip { enabled: false, .. }]
        ));
        assert!(matches!(
            held(&mut bench, "deck2.sample16.held")[..],
            [Command::TriggerSample { deck: 1, slot: 15, .. }, Command::StopSample { deck: 1, slot: 15 }]
        ));
        let bends: Vec<f64> =
            ["deck1.bend.up", "deck1.bend.down", "deck1.bend.up.small", "deck1.bend.down.small"]
                .into_iter()
                .flat_map(|id| held(&mut bench, id))
                .filter_map(|c| match c {
                    Command::SetPitchBend { deck: 0, amount } => Some(amount),
                    _ => None,
                })
                .collect();
        assert_eq!(bends, [0.03, 0.0, -0.03, 0.0, 0.01, 0.0, -0.01, 0.0]);
    }

    #[test]
    fn a_held_key_keeps_what_it_holds_in_the_decks_settings() {
        let mut bench = Bench::new();
        bench.press("deck2.eq.low.kill", 1.0).unwrap();
        bench.press("deck1.fx2.held", 1.0).unwrap();
        assert!(lock(&bench.state.decks)[1].strip.kills[0]);
        assert!(lock(&bench.state.decks)[0].fx.slots[1].enabled);
        bench.press("deck2.eq.low.kill", 0.0).unwrap();
        bench.press("deck1.fx2.held", 0.0).unwrap();
        assert!(!lock(&bench.state.decks)[1].strip.kills[0]);
        assert!(!lock(&bench.state.decks)[0].fx.slots[1].enabled);
        assert!(matches!(
            bench.sent()[..],
            [
                Command::SetEqKill { deck: 1, kill: true, .. },
                Command::SetFxEnabled { deck: 0, slot: 1, enabled: true },
                Command::SetEqKill { deck: 1, kill: false, .. },
                Command::SetFxEnabled { deck: 0, slot: 1, enabled: false },
            ]
        ));
    }

    #[test]
    fn a_pad_plays_its_sample_as_hard_as_it_is_hit() {
        let mut bench = Bench::new();
        bench.press("deck2.sample3.velocity", 0.25).unwrap();
        bench.press("deck2.sample3.velocity", 0.0).unwrap();
        assert!(matches!(bench.sent()[..], [Command::TriggerSample { deck: 1, slot: 2, gain: 0.25 }]));
    }

    #[test]
    fn a_deck_not_in_use_does_nothing() {
        let mut bench = Bench::new();
        bench.press("deck3.brake", 1.0).unwrap();
        assert!(bench.sent().is_empty());
    }

    #[test]
    fn a_loop_cleared_is_forgotten_by_the_engine() {
        let mut bench = Bench::new();
        crate::actions::transport::set_loop(&bench.state, 0, Some(1.0), Some(3.0), false).unwrap();
        let stored = bench.sent();
        assert!(matches!(
            stored[..],
            [Command::SetLoop { region: Some(LoopRegion { start_s: 1.0, end_s: 3.0 }), active: false, .. }]
        ));
    }

    #[test]
    fn every_action_rust_owns_is_performed_here() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/controllers/targets.json");
        let listed: Vec<prisme_control::ListedTarget> =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let measured = ["deck1.jump.-8", "deck2.loop.0.5", "deck4.key.+2", "deck1.sample3.velocity"];
        let mut bench = Bench::new();
        for id in listed.iter().map(|t| t.id.as_str()).chain(measured) {
            let target: Target = id.parse().unwrap();
            if target.owner() == Owner::Ui {
                continue;
            }
            for value in [1.0, 0.0] {
                let outcome = bench.perform(id, Input::Value(value), Source::Interface);
                let refused = not_performed(&target).to_string();
                assert!(!matches!(&outcome, Err(AppError::Other(e)) if *e == refused), "{id}");
            }
        }
        bench.sent();
    }
}

#[cfg(test)]
mod faders {
    use crate::actions::bench::Bench;
    use crate::actions::Source;
    use crate::dto::CrossfaderCurveDto;
    use crate::state::lock;
    use prisme_actions::input::Input;
    use prisme_actions::scales::eq_knob;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    /// `id` set to `value` on the screen.
    fn set(bench: &Bench, id: &str, value: f32) {
        bench.perform(id, Input::Value(value), Source::Interface).unwrap();
    }

    /// A 7-bit knob from full left to `to`.
    fn knob(to: f32) -> Vec<f32> {
        (0..=(to * 127.0).round() as u32).map(|i| i as f32 / 127.0).collect()
    }

    fn turn(bench: &Bench, id: &str, values: &[f32]) {
        for &value in values {
            bench.press(id, value).unwrap();
        }
    }

    #[test]
    fn a_fader_picks_its_setting_up_on_each_controller_apart() {
        let bench = Bench::new();
        let volume = || lock(&bench.state.decks)[0].volume;
        set(&bench, "deck1.volume", 0.5);
        bench.press("deck1.volume", 0.49).unwrap();
        assert_eq!(volume(), 0.49);
        bench.moved("Xone", "deck1.volume", Input::Value(0.1)).unwrap();
        assert_eq!(volume(), 0.49, "the other controller's fader is elsewhere");
        bench.press("deck1.volume", 0.6).unwrap();
        bench.moved("Xone", "deck1.volume", Input::Value(0.7)).unwrap();
        assert_eq!(volume(), 0.7, "crossed it");
        set(&bench, "deck1.volume", 0.2);
        bench.press("deck1.volume", 0.9).unwrap();
        assert_eq!(volume(), 0.2, "moved on the screen: picked up again");
    }

    #[test]
    fn an_encoder_moves_a_fader_from_where_it_stands_by_its_step_without_picking_it_up() {
        let bench = Bench::new();
        set(&bench, "deck1.volume", 0.5);
        bench.moved("DDJ", "deck1.volume", Input::Steps { steps: 2.0, step: None }).unwrap();
        assert!(close(lock(&bench.state.decks)[0].volume, 0.5 + 2.0 / 64.0));
        bench.moved("DDJ", "deck1.volume", Input::Steps { steps: -4.0, step: Some(0.25) }).unwrap();
        assert_eq!(lock(&bench.state.decks)[0].volume, 0.0, "within its travel");
    }

    #[test]
    fn an_eq_band_given_its_silent_end_stands_where_it_was_left() {
        let bench = Bench::new();
        set(&bench, "deck1.eq.low", 0.01);
        assert_eq!(lock(&bench.state.decks)[0].strip.eq[0], 0.0, "silent");
        assert_eq!(bench.position("deck1.eq.low"), Some(0.01));
        bench.press("deck1.eq.low", 0.03).unwrap();
        assert!(lock(&bench.state.decks)[0].strip.eq[0] > 0.0, "a controller's knob there picks it up");
        assert_eq!(bench.position("deck2.eq.low"), Some(eq_knob(1.0)), "the other deck's");
    }

    #[test]
    fn an_eq_band_and_a_live_input_come_back_from_their_silent_end() {
        let bench = Bench::new();
        set(&bench, "deck1.eq.low", 0.0);
        set(&bench, "mixer.mic", 0.0);
        turn(&bench, "deck1.eq.low", &knob(0.5));
        turn(&bench, "mixer.mic", &knob(0.75));
        assert!((lock(&bench.state.decks)[0].strip.eq[0] - 1.0).abs() < 0.1);
        assert!((bench.state.preferences.read(|p| p.mixer.mic.level) - 1.0).abs() < 0.1);
        // silent, an encoder brings it back
        set(&bench, "deck2.eq.mid", 0.0);
        for _ in 0..4 {
            bench.moved("DDJ", "deck2.eq.mid", Input::Steps { steps: 1.0, step: None }).unwrap();
        }
        assert!(lock(&bench.state.decks)[1].strip.eq[1] > 0.0);
    }

    #[test]
    fn the_crossfader_curve_switches_along_a_knob_both_ways() {
        let bench = Bench::new();
        let curve = || bench.state.preferences.read(|p| p.mixer.crossfader_curve);
        turn(&bench, "mixer.crossfaderCurve", &knob(1.0));
        assert_eq!(curve(), CrossfaderCurveDto::Sharp);
        let back: Vec<f32> = knob(1.0).into_iter().rev().collect();
        turn(&bench, "mixer.crossfaderCurve", &back);
        assert_eq!(curve(), CrossfaderCurveDto::Smooth);
    }

    #[test]
    fn the_period_and_the_effect_move_along_a_knob_keeping_a_controller_within_a_step() {
        let bench = Bench::new();
        let slot = || lock(&bench.state.decks)[0].fx.slots[2];
        assert!(close(bench.position("deck1.fx3.period").unwrap(), 3.5 / 7.0));
        set(&bench, "deck1.fx3.period", 1.0);
        assert_eq!(slot().beats, 8.0);
        set(&bench, "deck1.fx3.period", 0.2);
        assert_eq!(slot().beats, 0.25);
        assert_eq!(bench.position("deck1.fx3.period"), Some(0.2));
        crate::actions::fx::set_fx_beats(&bench.state, 0, 2, 2.0).unwrap();
        assert!(close(bench.position("deck1.fx3.period").unwrap(), 4.5 / 7.0), "changed elsewhere");

        set(&bench, "deck1.fx3.kind", 0.99);
        assert_eq!(slot().kind, crate::dto::EffectKindDto::Bitcrusher);
        set(&bench, "deck1.fx3.kind", 0.4);
        assert_eq!(slot().kind, crate::dto::EffectKindDto::Flanger);
        assert_eq!(bench.position("deck1.fx3.kind"), Some(0.4));
    }

    #[test]
    fn the_faders_set_their_settings_on_their_scales() {
        let bench = Bench::new();
        set(&bench, "mixer.headphones", 1.0);
        assert_eq!(bench.state.preferences.read(|p| p.mixer.headphone_gain), 1.5);
        assert_eq!(bench.position("mixer.headphones"), Some(1.0));
        set(&bench, "mixer.crossfader", 0.25);
        assert_eq!(*lock(&bench.state.crossfader), -0.5);
        set(&bench, "deck2.filter", 0.0);
        assert_eq!(lock(&bench.state.decks)[1].strip.filter, -1.0);
        set(&bench, "deck1.fx.super", 0.7);
        assert!(lock(&bench.state.decks)[0].fx.slots.iter().all(|s| s.amount == 0.7));
        set(&bench, "sampler.gain", 0.3);
        let gains = lock(&bench.state.decks).map(|d| d.sampler_gain);
        assert_eq!(gains, [0.3, 0.3, 1.0, 1.0], "the decks in use");
        set(&bench, "deck1.fx2.mix", 0.6);
        assert_eq!(lock(&bench.state.decks)[0].fx.slots[1].mix, 0.6);
    }

    #[test]
    fn the_key_and_the_stems_of_no_track_and_the_place_in_none_do_nothing() {
        let mut bench = Bench::new();
        set(&bench, "deck1.key", 1.0);
        set(&bench, "deck1.vocals.level", 0.2);
        set(&bench, "deck1.position", 0.5);
        assert!(bench.sent().is_empty());
    }
}
