//! What each action does, by its target, with what its control gives: a key pressed or let go,
//! a fader's position, an encoder's steps.

use super::{deck, fx, mixer, not_performed, sampler, transport, Performing};
use crate::error::AppResult;
use crate::state::lock;
use prisme_actions::input::{Given, Input};
use prisme_actions::scales::Tempo;
use prisme_control::{AutoDjAction, Beats, DeckAction, FxAction, SampleAction, Target};
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
            Target::Deck(deck, action) => self.deck(deck, action, given),
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
            DeckAction::Pitch => {
                let Given::Value(position) = given else { return Ok(()) };
                let tempo = Tempo { ratio: lock(&state.decks)[deck].pitch, range: state.tempo_range(deck) };
                deck::set_pitch(state, deck, tempo.at(position))
            }
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
