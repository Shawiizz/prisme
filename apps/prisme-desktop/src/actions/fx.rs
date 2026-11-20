//! A deck's chain of beat effects: each slot's effect and settings, the chain on or off and the
//! part of the track it is on; the slot its three knobs are focused on.

use super::Performing;
use crate::dto::{EffectKindDto, FxDto, FxPartDto, SeparationDto};
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState};
use prisme_control::{DeckAction, Effect, FxAction};
use prisme_engine::{Command, FX_SLOTS};

/// Changes a slot of the deck's chain as the backend keeps it, then sends `command` to the
/// engine.
fn set_slot(
    state: &AppState,
    deck: usize,
    slot: usize,
    change: impl FnOnce(&mut FxDto),
    command: Command,
) -> AppResult<()> {
    AppState::check_deck(deck)?;
    if slot >= FX_SLOTS {
        return Err(AppError::Other(format!("no effect slot {}", slot + 1)));
    }
    change(&mut lock(&state.decks)[deck].fx.slots[slot]);
    state.send_if_running(command)
}

pub fn set_fx_enabled(state: &AppState, deck: usize, slot: usize, enabled: bool) -> AppResult<()> {
    set_slot(state, deck, slot, |fx| fx.enabled = enabled, Command::SetFxEnabled { deck, slot, enabled })
}

/// Another effect in the slot, on or off as it was.
pub fn set_fx(state: &AppState, deck: usize, slot: usize, kind: EffectKindDto) -> AppResult<()> {
    set_slot(state, deck, slot, |fx| fx.kind = kind, Command::SetFx { deck, slot, kind: kind.into() })
}

/// The level of the slot's effect, 0..1.
pub fn set_fx_mix(state: &AppState, deck: usize, slot: usize, mix: f32) -> AppResult<()> {
    set_slot(state, deck, slot, |fx| fx.mix = mix, Command::SetFxMix { deck, slot, mix })
}

/// The slot's main knob, 0..1.
pub fn set_fx_amount(state: &AppState, deck: usize, slot: usize, amount: f32) -> AppResult<()> {
    set_slot(state, deck, slot, |fx| fx.amount = amount, Command::SetFxAmount { deck, slot, amount })
}

/// The slot's second knob, 0..1.
pub fn set_fx_param2(state: &AppState, deck: usize, slot: usize, param2: f32) -> AppResult<()> {
    set_slot(state, deck, slot, |fx| fx.param2 = param2, Command::SetFxParam2 { deck, slot, param2 })
}

/// The slot's period in beats.
pub fn set_fx_beats(state: &AppState, deck: usize, slot: usize, beats: f32) -> AppResult<()> {
    set_slot(state, deck, slot, |fx| fx.beats = beats, Command::SetFxBeats { deck, slot, beats })
}

/// The whole chain on or off, each slot keeping its own switch.
pub fn set_fx_chain_enabled(state: &AppState, deck: usize, enabled: bool) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].fx.enabled = enabled;
    state.send_if_running(Command::SetFxChainEnabled { deck, enabled })
}

/// The part of the track the chain is on, where separated (`None`: the whole deck).
pub fn set_fx_part(state: &AppState, deck: usize, part: Option<FxPartDto>) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].fx.part = part;
    state.send_if_running(Command::SetFxParts { deck, parts: FxPartDto::parts(part) })
}

/// The parts a chain can be on alone, as its track is separated, after the whole deck.
fn parts(separation: SeparationDto) -> &'static [Option<FxPartDto>] {
    use FxPartDto::*;
    match separation {
        SeparationDto::None => &[None],
        SeparationDto::Vocals => &[None, Some(Vocals), Some(Instrumental)],
        SeparationDto::Full => &[None, Some(Vocals), Some(Drums), Some(Bass), Some(Other)],
    }
}

/// The chain of `deck` on the next part of its track it can be on, after the last on the whole
/// deck again.
pub fn next_fx_part(state: &AppState, deck: usize) -> AppResult<()> {
    let separation = state.with_track(deck, |t| t.separation()).unwrap_or(SeparationDto::None);
    let parts = parts(separation);
    let at = parts.iter().position(|p| *p == lock(&state.decks)[deck].fx.part);
    set_fx_part(state, deck, parts[at.map_or(0, |at| (at + 1) % parts.len())])
}

/// The effect `step` places after the slot's (before it, negative), on or off as it was.
pub(super) fn cycle_fx(state: &AppState, deck: usize, slot: usize, step: isize) -> AppResult<()> {
    let effect = Effect::from(lock(&state.decks)[deck].fx.slots[slot].kind);
    let at = Effect::NAMED.iter().position(|e| *e == effect).unwrap_or(0) as isize;
    let next = Effect::NAMED[(at + step).rem_euclid(Effect::NAMED.len() as isize) as usize];
    set_fx(state, deck, slot, next.into())
}

/// `effect` put in the slot and on; pressed again while it plays, off.
pub(super) fn press_fx(state: &AppState, deck: usize, slot: usize, effect: Effect) -> AppResult<()> {
    let fx = lock(&state.decks)[deck].fx.slots[slot];
    let kind = EffectKindDto::from(effect);
    if fx.kind == kind && fx.enabled {
        return set_fx_enabled(state, deck, slot, false);
    }
    if fx.kind != kind {
        set_fx(state, deck, slot, kind)?;
    }
    set_fx_enabled(state, deck, slot, true)
}

impl Performing<'_> {
    /// Slot `slot` of the chain of `deck` focused on the three knobs, or let go of.
    pub(super) fn focus_fx(&self, deck: usize, slot: usize) -> AppResult<()> {
        let focus = &mut self.perform_state().decks[deck].fx_focus;
        *focus = if *focus == Some(slot) { None } else { Some(slot) };
        Ok(())
    }

    /// Knob `knob` of the chain of `deck`, as Mixxx's effect unit has them: with no effect
    /// focused, the amount of effect `knob`; with one focused, its amount, second knob and
    /// period.
    pub(super) fn fx_knob(&self, deck: usize, knob: usize) -> DeckAction {
        match self.perform_state().decks[deck].fx_focus {
            None => DeckAction::Fx(knob, FxAction::Amount),
            Some(slot) => DeckAction::Fx(slot, [FxAction::Amount, FxAction::Param2, FxAction::Period][knob]),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::actions::bench::Bench;
    use crate::actions::Source;
    use crate::dto::{EffectKindDto, FxPartDto};
    use crate::state::lock;
    use prisme_actions::input::Input;

    fn press(bench: &Bench, id: &str) {
        bench.press(id, 1.0).unwrap();
        bench.press(id, 0.0).unwrap();
    }

    fn set(bench: &Bench, id: &str, value: f32) {
        bench.perform(id, Input::Value(value), Source::Interface).unwrap();
    }

    #[test]
    fn the_three_knobs_turn_the_amounts_or_the_focused_effect() {
        let bench = Bench::new();
        let slots = || lock(&bench.state.decks)[0].fx.slots;
        for (id, amount) in [("deck1.fx.amount", 0.1), ("deck1.fx2.amount", 0.2), ("deck1.fx3.amount", 0.3)] {
            set(&bench, id, amount);
        }
        let knobs = |bench: &Bench| [1, 2, 3].map(|k| bench.position(&format!("deck1.fx.knob{k}")).unwrap());
        assert_eq!(knobs(&bench), [0.1, 0.2, 0.3]);
        set(&bench, "deck1.fx.knob3", 0.9);
        assert_eq!(slots()[2].amount, 0.9);

        press(&bench, "deck1.fx2.focus");
        set(&bench, "deck1.fx.knob1", 0.6);
        set(&bench, "deck1.fx.knob2", 0.8);
        set(&bench, "deck1.fx.knob3", 0.0);
        let focused = slots()[1];
        assert_eq!((focused.amount, focused.param2, focused.beats), (0.6, 0.8, 0.125));
        assert_eq!(slots()[0].amount, 0.1);
        assert_eq!(knobs(&bench)[2], 0.0);

        press(&bench, "deck1.fx2.focus");
        assert_eq!(knobs(&bench)[1], 0.6, "let go: the amounts again");

        press(&bench, "deck1.fx3.focus");
        set(&bench, "deck1.fx.knob3", 0.2);
        assert_eq!(slots()[2].beats, 0.25);
        assert_eq!(knobs(&bench)[2], 0.2, "the period's own memory");
        crate::actions::fx::set_fx_beats(&bench.state, 0, 2, 2.0).unwrap();
        assert!((knobs(&bench)[2] - 4.5 / 7.0).abs() < 1e-6, "changed elsewhere");
    }

    #[test]
    fn a_controllers_knob_picks_the_focused_setting_up() {
        let bench = Bench::new();
        set(&bench, "deck1.fx2.param2", 0.9);
        bench.press("deck1.fx.knob2", 0.2).unwrap();
        bench.press("deck1.fx.knob2", 0.4).unwrap();
        press(&bench, "deck1.fx2.focus");
        bench.press("deck1.fx.knob2", 0.5).unwrap();
        assert_eq!(lock(&bench.state.decks)[0].fx.slots[1].param2, 0.9, "not picked up yet");
        bench.press("deck1.fx.knob2", 0.95).unwrap();
        assert_eq!(lock(&bench.state.decks)[0].fx.slots[1].param2, 0.95);
    }

    #[test]
    fn an_effect_key_puts_it_on_and_off_and_the_next_keys_go_round() {
        let bench = Bench::new();
        let slot = || lock(&bench.state.decks)[1].fx.slots[2];
        press(&bench, "deck2.fx3.reverb");
        assert_eq!((slot().kind, slot().enabled), (EffectKindDto::Reverb, true));
        press(&bench, "deck2.fx3.reverb");
        assert!(!slot().enabled, "pressed again: off");
        press(&bench, "deck2.fx3.previous");
        press(&bench, "deck2.fx3.previous");
        press(&bench, "deck2.fx3.previous");
        assert_eq!(slot().kind, EffectKindDto::Bitcrusher, "round past the first");
        assert!(!slot().enabled, "on or off as it was");
        press(&bench, "deck2.fx3.next");
        assert_eq!(slot().kind, EffectKindDto::Echo);
    }

    #[test]
    fn the_focus_goes_from_slot_to_slot_and_reaches_the_interface() {
        let bench = Bench::new();
        press(&bench, "deck1.fx3.focus");
        press(&bench, "deck1.fx.focus");
        assert_eq!(lock(&bench.state.perform).decks[0].fx_focus, Some(0));
        let pads = crate::dto::ControlStateDto::of(&bench.state, [crate::state::Section::Perform]).perform;
        assert_eq!(pads.unwrap()[0].fx_focus, Some(0));
        press(&bench, "deck1.fx.focus");
        assert_eq!(lock(&bench.state.perform).decks[0].fx_focus, None);
    }

    #[test]
    fn the_chain_goes_from_part_to_part_of_the_track_as_it_is_separated() {
        let bench = Bench::new();
        let part = |bench: &Bench| lock(&bench.state.decks)[0].fx.part;
        press(&bench, "deck1.fx.part");
        assert_eq!(part(&bench), None, "no track: the whole deck");
        bench.load(0, 1.0, None);
        lock(&bench.state.tracks)[0].as_mut().unwrap().stems[prisme_engine::Stem::Vocals as usize] =
            Some(std::sync::Arc::new(prisme_media::TrackAudio::from_interleaved(48_000, vec![0.0; 2])));
        press(&bench, "deck1.fx.part");
        assert_eq!(part(&bench), Some(FxPartDto::Vocals));
        press(&bench, "deck1.fx.part");
        assert_eq!(part(&bench), Some(FxPartDto::Instrumental));
        press(&bench, "deck1.fx.part");
        assert_eq!(part(&bench), None);
    }
}
