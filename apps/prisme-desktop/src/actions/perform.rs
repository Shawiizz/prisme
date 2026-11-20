//! What each action does, by its target, with what its control gives: a key pressed or let go,
//! a fader's position, an encoder's steps.

use super::{deck, fx, general, mixer, not_performed, sampler, transport, Performing};
use crate::dto::{CrossfaderCurveDto, EffectKindDto, LiveSourceDto};
use crate::error::AppResult;
use crate::state::lock;
use prisme_actions::input::{Given, Input};
use prisme_actions::pads::ROLL_BEATS;
use prisme_actions::position::FX_PERIODS;
use prisme_actions::scales::{
    balance_amount, eq_gain, eq_knob, filter_amount, headphone_gain, live_gain, live_knob, next_tempo_range,
    trim_gain, Tempo,
};
use prisme_control::{
    AutoDjAction, Beats, DeckAction, DeckSwitch, Effect, FxAction, GeneralAction, Kind, MixerAction,
    MixerSwitch, SampleAction, Target, Turn, FX_SLOTS,
};
use prisme_engine::Command;
use transport::{LOOP_BEATS, PITCH_BEND, SMALL_BEND, TEMPO_STEP};

impl Performing<'_> {
    /// Performs `target` driven by `input`: nothing on a deck not in use, nor for a light.
    pub(super) fn perform(&self, target: &Target, input: Input) -> AppResult<()> {
        if target.kind() == Kind::Light
            || target.deck().is_some_and(|deck| self.state.has_deck(deck).is_err())
        {
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
            Target::Library(action) if given.pressed() => self.load(action),
            Target::Library(_) => Ok(()),
            Target::General(action) => self.general(action, given),
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

    /// An action of `deck` but its faders': the keys held act as they are pressed and let go,
    /// the others as they are pressed; the encoders by their steps.
    fn deck(&self, deck: usize, action: DeckAction, given: Given) -> AppResult<()> {
        let state = self.state;
        let held = given.pressed();
        let send = |command| transport::send(state, deck, command);
        let bend =
            |amount: f64| send(Command::SetPitchBend { deck, amount: if held { amount } else { 0.0 } });
        let steps = match given {
            Given::Steps(steps) => steps,
            Given::Value(_) => 0,
        };
        let size = || lock(&state.decks)[deck].size_beats;
        let key = || state.with_track(deck, |t| i32::from(t.key_shift)).unwrap_or(0);
        match action {
            DeckAction::Spinback => send(Command::SpinBack { deck, held }),
            DeckAction::Censor => send(Command::Censor { deck, active: held }),
            DeckAction::ReverseHeld => send(Command::HoldReverse { deck, held }),
            DeckAction::SlipHeld => send(Command::SetSlip { deck, enabled: held }),
            DeckAction::BendDown => bend(-PITCH_BEND),
            DeckAction::BendUp => bend(PITCH_BEND),
            DeckAction::BendDownSmall => bend(-SMALL_BEND),
            DeckAction::BendUpSmall => bend(SMALL_BEND),
            DeckAction::EqKill(band) => mixer::set_eq_kill(state, deck, band.into(), held),
            DeckAction::Fx(slot, FxAction::Held) => fx::set_fx_enabled(state, deck, slot, held),
            DeckAction::Sample(slot, SampleAction::Held) if held => {
                sampler::trigger_sample(state, deck, slot, None)
            }
            DeckAction::Sample(slot, SampleAction::Held) => sampler::stop_sample(state, deck, slot),
            DeckAction::Cue => self.cue(deck, held),
            DeckAction::Roll(roll) => self.roll(deck, ROLL_BEATS[roll], held),
            DeckAction::RollBeats(Beats(beats)) => self.roll(deck, beats, held),
            DeckAction::Slice(slice) => self.slice(deck, slice, held),
            DeckAction::Cut(cut) => self.cut(deck, cut, held),
            DeckAction::SearchBack => self.search(deck, -1.0, held),
            DeckAction::SearchForward => self.search(deck, 1.0, held),
            DeckAction::GridMove => self.edit_grid(deck, action, given),
            DeckAction::Tempo => self.step_tempo(deck, steps),
            DeckAction::Size if steps != 0 => deck::scale_size(state, deck, 2f64.powi(steps)),
            DeckAction::Fx(slot, FxAction::Beats) if steps != 0 => {
                let (shortest, longest) = (FX_PERIODS[0], FX_PERIODS[FX_PERIODS.len() - 1]);
                let beats = lock(&state.decks)[deck].fx.slots[slot].beats * 2f32.powi(steps);
                fx::set_fx_beats(state, deck, slot, beats.clamp(shortest, longest))
            }
            DeckAction::LoopMove if steps != 0 => self.move_loop(deck, steps),
            DeckAction::Size | DeckAction::Fx(_, FxAction::Beats) | DeckAction::LoopMove => Ok(()),
            // a key let go
            _ if !held && action.kind() == Kind::Button => Ok(()),
            DeckAction::Play => self.play(deck),
            DeckAction::CuePlay => self.to_cue(deck, Some(true)),
            DeckAction::CueStop => self.to_cue(deck, Some(false)),
            DeckAction::CueGoto => self.to_cue(deck, None),
            DeckAction::CueSet => self.set_cue_here(deck),
            DeckAction::Start => self.to(deck, 0.0, Some(false)),
            DeckAction::StartPlay => self.to(deck, 0.0, Some(true)),
            DeckAction::StartKeep => self.to(deck, 0.0, None),
            DeckAction::End => match state.deck_status(deck) {
                Ok(status) if status.duration_s > 0.0 => self.seek(deck, status.duration_s),
                _ => Ok(()),
            },
            DeckAction::HotCue(slot) => self.hot_cue(deck, slot),
            DeckAction::Fx(
                slot,
                action @ (FxAction::Next | FxAction::Previous | FxAction::Effect(_) | FxAction::Focus),
            ) => self.fx_key(deck, slot, action),
            DeckAction::FxPart => fx::next_fx_part(state, deck),
            DeckAction::FilterToggle => self.toggle_filter(deck),
            DeckAction::Tap => self.tap(deck),
            DeckAction::KeySync => self.sync_key(deck),
            DeckAction::Stem(part) => deck::toggle_stem(state, deck, part),
            DeckAction::Brake => send(Command::Brake { deck }),
            DeckAction::LoopIn => send(Command::LoopIn { deck }),
            DeckAction::LoopOut => send(Command::LoopOut { deck }),
            DeckAction::Reloop => send(Command::Reloop { deck }),
            DeckAction::LoopHalve => send(Command::ScaleLoop { deck, factor: 0.5 }),
            DeckAction::LoopDouble => send(Command::ScaleLoop { deck, factor: 2.0 }),
            DeckAction::LoopBeats(Beats(beats)) => send(Command::BeatLoop { deck, beats }),
            DeckAction::Loop => self.toggle_loop(deck, LOOP_BEATS),
            DeckAction::LoopSize => self.toggle_loop(deck, size()),
            DeckAction::LoopClear => transport::set_loop(state, deck, None, None, false),
            DeckAction::ReloopStop => self.reloop_stopped(deck),
            DeckAction::LoopInGoto => self.to_loop_edge(deck, false),
            DeckAction::LoopOutGoto => self.to_loop_edge(deck, true),
            DeckAction::LoopMoveBack => self.move_loop(deck, -1),
            DeckAction::LoopMoveForward => self.move_loop(deck, 1),
            DeckAction::JumpBeats(Beats(beats)) => self.jump(deck, beats),
            DeckAction::JumpBack => self.jump(deck, -size()),
            DeckAction::JumpForward => self.jump(deck, size()),
            DeckAction::SizeHalve => deck::scale_size(state, deck, 0.5),
            DeckAction::SizeDouble => deck::scale_size(state, deck, 2.0),
            DeckAction::SyncLeader => deck::toggle_leader(state, deck),
            DeckAction::SyncOnce => deck::match_once(state, deck),
            DeckAction::Switch(switch, turn) => self.switch(deck, switch, turn),
            DeckAction::TempoReset => deck::set_pitch(state, deck, 1.0),
            DeckAction::TempoUp => self.step_tempo(deck, 1),
            DeckAction::TempoDown => self.step_tempo(deck, -1),
            DeckAction::TempoRange => self.next_tempo_range(Some(deck), 1),
            DeckAction::TempoRangePrevious => self.next_tempo_range(Some(deck), -1),
            DeckAction::KeyUp => deck::key_to(state, deck, key() + 1),
            DeckAction::KeyDown => deck::key_to(state, deck, key() - 1),
            DeckAction::KeyReset => deck::key_to(state, deck, 0),
            DeckAction::KeyTo(semitones) => deck::key_to(state, deck, semitones),
            DeckAction::HeadphonesSolo => mixer::solo_cue(state, deck),
            DeckAction::CrossfaderSide(side) => mixer::set_crossfader_side(state, deck, side.into()),
            DeckAction::Fx(slot, FxAction::Enabled(turn)) => {
                let on = lock(&state.decks)[deck].fx.slots[slot].enabled;
                fx::set_fx_enabled(state, deck, slot, turn.turns(on))
            }
            DeckAction::Sample(slot, SampleAction::Trigger) => {
                sampler::trigger_sample(state, deck, slot, None)
            }
            // as hard as the pad was hit
            DeckAction::Sample(slot, SampleAction::Velocity) => match given {
                Given::Value(velocity) => sampler::trigger_sample(state, deck, slot, Some(velocity)),
                Given::Steps(_) => Ok(()),
            },
            DeckAction::Sample(slot, SampleAction::Stop) => sampler::stop_sample(state, deck, slot),
            DeckAction::Sample(slot, SampleAction::Load) => self.load_sample(deck, slot),
            DeckAction::Sample(slot, SampleAction::Eject) => sampler::clear_sample(state, deck, slot),
            DeckAction::Sample(slot, SampleAction::Smart) => self.smart_sample(deck, slot),
            DeckAction::Eject => self.eject(deck),
            DeckAction::HotCueDelete(slot) => self.delete_hot_cue(deck, slot),
            DeckAction::GridHere
            | DeckAction::GridEarlier
            | DeckAction::GridLater
            | DeckAction::GridFaster
            | DeckAction::GridSlower => self.edit_grid(deck, action, given),
            _ => Err(not_performed(&Target::Deck(deck, action))),
        }
    }

    /// A switch of `deck` turned `turn`, from where it stands.
    fn switch(&self, deck: usize, switch: DeckSwitch, turn: Turn) -> AppResult<()> {
        let state = self.state;
        let d = lock(&state.decks)[deck];
        let status = || state.deck_status(deck).unwrap_or_default();
        let send = |command| transport::send(state, deck, command);
        match switch {
            DeckSwitch::Sync => deck::set_sync(state, deck, turn.turns(status().sync.locked)),
            DeckSwitch::Slip => send(Command::SetSlip { deck, enabled: turn.turns(status().slip_mode) }),
            DeckSwitch::Reverse => send(Command::SetReverse { deck, enabled: turn.turns(status().reverse) }),
            DeckSwitch::Repeat => deck::set_repeat(state, deck, turn.turns(d.repeat)),
            DeckSwitch::FaderStart => mixer::set_fader_start(state, deck, turn.turns(d.fader_start)),
            DeckSwitch::KeyLock => deck::set_key_lock(state, deck, turn.turns(d.key_lock)),
            DeckSwitch::Headphones => mixer::set_deck_cue(state, deck, turn.turns(d.cue)),
            DeckSwitch::Mute => mixer::set_mute(state, deck, turn.turns(d.mute)),
            DeckSwitch::Passthrough => deck::set_passthrough(state, deck, turn.turns(d.passthrough)),
            DeckSwitch::Vinyl => deck::set_vinyl_enabled(state, deck, turn.turns(d.vinyl_enabled)),
            DeckSwitch::JogScratch => deck::set_jog_scratch(state, deck, turn.turns(d.jog_scratch)),
            DeckSwitch::FxChain => fx::set_fx_chain_enabled(state, deck, turn.turns(d.fx.enabled)),
        }
    }

    /// So many beats ahead, or back below zero.
    fn jump(&self, deck: usize, beats: f64) -> AppResult<()> {
        transport::beat_jump(self.state, self.auto_dj().as_deref(), deck, beats)
    }

    /// A loop of `beats` where `deck` plays, or out of the loop it plays.
    fn toggle_loop(&self, deck: usize, beats: f64) -> AppResult<()> {
        let looping = self.state.deck_status(deck).is_ok_and(|status| status.looping.active);
        let command = if looping { Command::ExitLoop { deck } } else { Command::BeatLoop { deck, beats } };
        transport::send(self.state, deck, command)
    }

    /// The tempo of `deck` moved by so many steps within its fader's range, and kept: on the
    /// step, so that steps back bring it to the track's own again.
    fn step_tempo(&self, deck: usize, steps: i32) -> AppResult<()> {
        let range = self.state.tempo_range(deck);
        let ratio = lock(&self.state.decks)[deck].pitch + f64::from(steps) * TEMPO_STEP;
        let stepped = (ratio / TEMPO_STEP).round() * TEMPO_STEP;
        deck::set_pitch(self.state, deck, stepped.clamp(1.0 - range, 1.0 + range))
    }

    /// The range of the tempo fader of `deck`, or of them all (`None`), `step` places on.
    fn next_tempo_range(&self, deck: Option<usize>, step: isize) -> AppResult<()> {
        let range = match deck {
            Some(deck) => self.state.tempo_range(deck),
            None => self.state.preferences.read(|p| p.dj.pitch_range),
        };
        deck::set_tempo_range(self.state, deck, next_tempo_range(range, step))
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
                self.perform_state().faders.give_silent_end(&target, at, eq_knob(gain));
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
            DeckAction::FxKnob(knob) => self.deck_fader(deck, self.fx_knob(deck, knob), at),
            DeckAction::FxSuper => {
                (0..FX_SLOTS).try_for_each(|slot| fx::set_fx_amount(state, deck, slot, at))
            }
            DeckAction::Fx(slot, FxAction::Mix) => fx::set_fx_mix(state, deck, slot, at),
            DeckAction::Fx(slot, FxAction::Amount) => fx::set_fx_amount(state, deck, slot, at),
            DeckAction::Fx(slot, FxAction::Param2) => fx::set_fx_param2(state, deck, slot, at),
            DeckAction::Fx(slot, FxAction::Period) => {
                let step = self.perform_state().faders.give_stepped(&target, at, FX_PERIODS.len());
                let beats = FX_PERIODS[step];
                if lock(&state.decks)[deck].fx.slots[slot].beats == beats {
                    return Ok(());
                }
                fx::set_fx_beats(state, deck, slot, beats)
            }
            DeckAction::Fx(slot, FxAction::Kind) => {
                let step = self.perform_state().faders.give_stepped(&target, at, Effect::NAMED.len());
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
        let mixer = state.preferences.read(|p| p.mixer);
        let switches = *lock(&state.live);
        let live = |source: LiveSourceDto| {
            let gain = live_gain(at);
            mixer::set_live_level(state, source, gain)?;
            self.perform_state().faders.give_silent_end(&target, at, live_knob(gain));
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
                let sharp = self.perform_state().faders.give_stepped(&target, at, 2) == 1;
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
            // a key let go
            _ if !given.pressed() => Ok(()),
            MixerAction::MicOn => mixer::set_live_on(state, LiveSourceDto::Mic, !switches[0].on),
            MixerAction::AuxOn => mixer::set_live_on(state, LiveSourceDto::Aux, !switches[1].on),
            MixerAction::MicCue => mixer::set_live_cue(state, LiveSourceDto::Mic, !switches[0].cue),
            MixerAction::AuxCue => mixer::set_live_cue(state, LiveSourceDto::Aux, !switches[1].cue),
            MixerAction::CrossfaderCurveNext => {
                let sharp = mixer.crossfader_curve == CrossfaderCurveDto::Sharp;
                let curve = if sharp { CrossfaderCurveDto::Smooth } else { CrossfaderCurveDto::Sharp };
                mixer::set_crossfader_curve(state, curve)
            }
            MixerAction::Switch(MixerSwitch::HeadphonesSplit, turn) => {
                mixer::set_split_cue(state, turn.turns(mixer.split_cue))
            }
            MixerAction::Switch(MixerSwitch::Talkover, turn) => {
                mixer::set_talkover(state, turn.turns(mixer.talkover))
            }
            MixerAction::Switch(MixerSwitch::CrossfaderReverse, turn) => {
                mixer::set_crossfader_reverse(state, turn.turns(mixer.crossfader_reverse))
            }
            MixerAction::MainClip => Err(not_performed(&target)),
        }
    }

    /// An action of every deck, given `given`.
    fn general(&self, action: GeneralAction, given: Given) -> AppResult<()> {
        let state = self.state;
        match action {
            _ if !given.pressed() => Ok(()),
            GeneralAction::Quantize(turn) => {
                general::set_quantize(state, turn.turns(state.preferences.read(|p| p.dj.quantize)))
            }
            GeneralAction::TempoRange => self.next_tempo_range(None, 1),
            GeneralAction::TempoRangePrevious => self.next_tempo_range(None, -1),
            GeneralAction::Record => self.record(),
            _ => Err(not_performed(&Target::General(action))),
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
    fn the_interface_hears_whole_steps_by_controller_a_keys_value_and_no_encoder_on_a_key() {
        let bench = Bench::new();
        let heard =
            |device, id: &str, input| bench.actions.interface_value(device, &id.parse().unwrap(), input);
        let half = Input::Steps { steps: 0.5, step: None };
        assert_eq!(heard("DDJ", "library.browse", half), None);
        assert_eq!(heard("Xone", "library.browse", half), None, "its own steps");
        assert_eq!(heard("DDJ", "library.browse", half), Some(1.0));
        assert_eq!(heard("DDJ", "library.lists", Input::Steps { steps: -2.0, step: None }), Some(-2.0));
        assert_eq!(heard("DDJ", "library.load1", Input::Value(0.3)), Some(0.3));
        assert_eq!(heard("DDJ", "library.load1", Input::Steps { steps: 1.0, step: None }), None);
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

#[cfg(test)]
mod switches {
    use crate::actions::bench::Bench;
    use crate::dto::CrossfaderCurveDto;
    use crate::state::{lock, LoadedTrack};
    use crate::track_waveform::TrackWaveform;
    use prisme_actions::input::Input;
    use prisme_engine::Command;
    use prisme_media::TrackAudio;
    use std::sync::Arc;

    /// `id` pressed then let go.
    fn press(bench: &Bench, id: &str) {
        bench.press(id, 1.0).unwrap();
        bench.press(id, 0.0).unwrap();
    }

    fn turn(bench: &Bench, id: &str, steps: f32) {
        bench.moved("DDJ", id, Input::Steps { steps, step: None }).unwrap();
    }

    #[test]
    fn a_switch_turned_on_or_off_stays_so_and_one_turned_over_flips() {
        let mut bench = Bench::new();
        let key_lock = |bench: &Bench| lock(&bench.state.decks)[0].key_lock;
        press(&bench, "deck1.keylock.on");
        press(&bench, "deck1.keylock.on");
        assert!(key_lock(&bench));
        press(&bench, "deck1.keylock");
        assert!(!key_lock(&bench));
        press(&bench, "deck1.keylock.off");
        assert!(!key_lock(&bench));

        press(&bench, "mixer.talkover.on");
        press(&bench, "mixer.talkover.on");
        assert!(bench.state.preferences.read(|p| p.mixer.talkover));
        press(&bench, "general.quantize.off");
        press(&bench, "general.quantize.off");
        assert!(!bench.state.preferences.read(|p| p.dj.quantize));
        press(&bench, "deck2.fx2.on");
        press(&bench, "deck2.fx2.on");
        assert!(lock(&bench.state.decks)[1].fx.slots[1].enabled);
        press(&bench, "deck2.fx2");
        assert!(!lock(&bench.state.decks)[1].fx.slots[1].enabled);
        bench.sent();

        // the engine's own: on twice is on
        press(&bench, "deck1.slip.on");
        press(&bench, "deck1.slip.on");
        assert!(matches!(
            bench.sent()[..],
            [Command::SetSlip { deck: 0, enabled: true }, Command::SetSlip { deck: 0, enabled: true }]
        ));
    }

    #[test]
    fn the_size_is_halved_and_doubled_within_a_sixteenth_and_64_beats() {
        let bench = Bench::new();
        let size = || lock(&bench.state.decks)[0].size_beats;
        turn(&bench, "deck1.size", 3.0);
        assert_eq!(size(), 32.0);
        for _ in 0..4 {
            press(&bench, "deck1.size.double");
        }
        assert_eq!(size(), 64.0);
        turn(&bench, "deck1.size", -20.0);
        assert_eq!(size(), 1.0 / 16.0);
        press(&bench, "deck1.size.halve");
        assert_eq!(size(), 1.0 / 16.0);
        assert_eq!(bench.state.preferences.read(|p| p.decks[0].size_beats), 1.0 / 16.0, "kept");
    }

    #[test]
    fn a_jump_and_a_sized_loop_go_by_the_size_the_loop_key_by_four_beats() {
        let mut bench = Bench::new();
        press(&bench, "deck2.size.double");
        press(&bench, "deck2.jump.back");
        press(&bench, "deck2.loop.size");
        press(&bench, "deck2.loop");
        assert!(matches!(
            bench.sent()[..],
            [
                Command::BeatJump { deck: 1, beats: -8.0 },
                Command::BeatLoop { deck: 1, beats: 8.0 },
                Command::BeatLoop { deck: 1, beats: 4.0 },
            ]
        ));
    }

    #[test]
    fn the_tempo_moves_by_a_tenth_of_a_percent_a_step_and_back_to_the_track() {
        let bench = Bench::new();
        let pitch = || lock(&bench.state.decks)[0].pitch;
        press(&bench, "deck1.tempo.up");
        press(&bench, "deck1.tempo.up");
        assert!((pitch() - 1.002).abs() < 1e-9);
        turn(&bench, "deck1.tempo", -5.0);
        assert!((pitch() - 0.997).abs() < 1e-9);
        turn(&bench, "deck1.tempo", 3.0);
        assert_eq!(pitch(), 1.0, "on the step");
        turn(&bench, "deck1.tempo", 200.0);
        assert_eq!(pitch(), 1.08, "within the fader's range");
        press(&bench, "deck1.tempo.reset");
        assert_eq!(pitch(), 1.0);
    }

    #[test]
    fn a_tempo_range_goes_round_on_a_deck_or_on_them_all() {
        let bench = Bench::new();
        press(&bench, "deck2.tempoRange");
        assert_eq!(bench.state.tempo_range(1), 0.16);
        press(&bench, "deck2.tempoRange.previous");
        press(&bench, "deck2.tempoRange.previous");
        assert_eq!(bench.state.tempo_range(1), 0.5);
        press(&bench, "general.tempoRange.previous");
        assert_eq!(bench.state.preferences.read(|p| p.dj.pitch_range), 0.5);
        assert_eq!(bench.state.tempo_range(1), 0.5, "every deck follows the general one again");
    }

    fn track() -> LoadedTrack {
        LoadedTrack {
            load_id: LoadedTrack::next_load_id(),
            path: "drop.flac".into(),
            audio: Arc::new(TrackAudio::from_interleaved(48_000, vec![0.0; 2])),
            waveform: TrackWaveform::growing(1),
            detected: Default::default(),
            edits: Default::default(),
            tags: Default::default(),
            cues: Vec::new(),
            key_shift: 0,
            stems: Default::default(),
            stem_mix: Default::default(),
            loudness: None,
        }
    }

    #[test]
    fn the_key_moves_by_a_semitone_within_twelve_either_way() {
        let mut bench = Bench::new();
        press(&bench, "deck1.key.up");
        assert!(bench.sent().is_empty(), "no track");
        lock(&bench.state.tracks)[0] = Some(track());
        let key = |bench: &Bench| bench.state.with_track(0, |t| t.key_shift).unwrap();
        press(&bench, "deck1.key.up");
        press(&bench, "deck1.key.up");
        assert_eq!(key(&bench), 2);
        press(&bench, "deck1.key.down");
        assert_eq!(key(&bench), 1);
        press(&bench, "deck1.key.+11");
        press(&bench, "deck1.key.up");
        assert_eq!(key(&bench), 12);
        press(&bench, "deck1.key.reset");
        assert_eq!(key(&bench), 0);
    }

    #[test]
    fn the_headphones_on_one_deck_alone_and_the_mixer_keys() {
        let bench = Bench::new();
        press(&bench, "deck2.headphones.on");
        press(&bench, "deck1.headphones.solo");
        assert_eq!(lock(&bench.state.decks).map(|d| d.cue)[..2], [true, false]);
        press(&bench, "mixer.mic.on");
        assert!(lock(&bench.state.live)[0].on);
        press(&bench, "mixer.mic.on");
        assert!(!lock(&bench.state.live)[0].on, "a toggle");
        press(&bench, "mixer.crossfaderCurve.next");
        assert_eq!(bench.state.preferences.read(|p| p.mixer.crossfader_curve), CrossfaderCurveDto::Sharp);
        press(&bench, "deck1.crossfader.right");
        assert_eq!(lock(&bench.state.decks)[0].side, Some(crate::dto::CrossfaderSideDto::Right));
    }

    #[test]
    fn sync_locks_a_deck_or_matches_a_record_once() {
        let mut bench = Bench::new();
        press(&bench, "deck1.sync.on");
        lock(&bench.state.decks)[1].vinyl_enabled = true;
        press(&bench, "deck2.sync");
        press(&bench, "deck1.sync.once");
        assert!(matches!(
            bench.sent()[..],
            [
                Command::SetSyncLocked { deck: 0, locked: true },
                Command::SyncOnce { deck: 1 },
                Command::SyncOnce { deck: 0 },
            ]
        ));
    }

    #[test]
    fn a_deck_matched_once_takes_the_tempo_of_the_other_on_its_fader_but_on_a_record() {
        let mut bench = Bench::new();
        bench.load(0, 10.0, Some(125.0));
        bench.load(1, 10.0, Some(128.0));
        bench.sent();
        press(&bench, "deck1.sync.once");
        assert!((lock(&bench.state.decks)[0].pitch - 1.024).abs() < 1e-9);
        assert!(matches!(
            bench.sent()[..],
            [Command::SetPitch { deck: 0, .. }, Command::SyncOnce { deck: 0 }]
        ));
        lock(&bench.state.decks)[1].vinyl_enabled = true;
        press(&bench, "deck2.sync.once");
        assert_eq!(lock(&bench.state.decks)[1].pitch, 1.0, "the record keeps its tempo");
        assert!(matches!(bench.sent()[..], [Command::SyncOnce { deck: 1 }]));
    }

    #[test]
    fn an_effects_period_is_halved_or_doubled_within_an_eighth_and_eight_beats() {
        let bench = Bench::new();
        let beats = || lock(&bench.state.decks)[0].fx.slots[2].beats;
        turn(&bench, "deck1.fx3.beats", 2.0);
        assert_eq!(beats(), 4.0);
        turn(&bench, "deck1.fx3.beats", 5.0);
        assert_eq!(beats(), 8.0);
        turn(&bench, "deck1.fx3.beats", -9.0);
        assert_eq!(beats(), 0.125);
    }
}
