//! A deck's own actions: the jog wheels, played in time (a jog searching moves through the
//! track quickly, under shift say), the tempo fader, and the deck leading the others.

use super::{take_over, Performing};
use crate::auto_dj::Takeover;
use crate::dto::StemMixDto;
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState, LoadedTrack};
use prisme_actions::input::Input;
use prisme_control::{DeckAction, StemPart};
use prisme_engine::Command;

/// Seconds of track a turn of a jog wheel goes through in a search.
const SEARCH_SECONDS_PER_TURN: f64 = 10.0;
/// How far a key moves, either way.
pub const MAX_KEY_SHIFT: u8 = 12;

/// A jog wheel of `deck` turned, touched or let go (`action`) by `input`, its turns not
/// added up as an encoder's steps: they come as finely as the wheel tells them.
pub(super) fn turn(doing: &Performing, deck: usize, action: DeckAction, input: Input) -> AppResult<()> {
    let steps_per_turn = match doing.source {
        super::Source::Controller(moved) => moved.jog_steps_per_turn,
        super::Source::Interface => prisme_control::JOG_STEPS_PER_TURN,
    };
    let jog_scratch = lock(&doing.state.decks)[deck].jog_scratch;
    let Some(command) = jog(deck, action, input, doing.at_s(), steps_per_turn, jog_scratch) else {
        return Ok(());
    };
    take_over(doing.auto_dj().as_deref(), Takeover::Transport(deck));
    doing.state.send_if_running(command)
}

/// The command for a jog wheel of `deck` turned or touched by `input` at `at_s`, a turn of it
/// sending `steps_per_turn`, its top holding the record or not (`jog_scratch`). None for a
/// touch of a wheel that only nudges.
fn jog(
    deck: usize,
    action: DeckAction,
    input: Input,
    at_s: f64,
    steps_per_turn: f64,
    jog_scratch: bool,
) -> Option<Command> {
    let turns = f64::from(input.amount()) / steps_per_turn;
    match action {
        DeckAction::Jog => Some(Command::Jog { deck, turns, at_s }),
        // however lightly: some wheels tell how hard they are touched; a wheel that only
        // nudges: its top is no hand on the record
        DeckAction::JogTouch => {
            jog_scratch.then_some(Command::JogTouch { deck, touched: input.amount() > 0.0, at_s })
        }
        DeckAction::JogSearch => Some(Command::Jump { deck, seconds: turns * SEARCH_SECONDS_PER_TURN }),
        _ => None,
    }
}

/// The tempo of `deck`'s fader, as a ratio to its track's; kept as the audio stops.
pub fn set_pitch(state: &AppState, deck: usize, ratio: f64) -> AppResult<()> {
    state.has_deck(deck)?;
    lock(&state.decks)[deck].pitch = ratio;
    state.send_if_running(Command::SetPitch { deck, ratio })
}

/// The key of the track on `deck` moved by `semitones`, until the next load.
pub fn set_key_shift(state: &AppState, deck: usize, semitones: i8) -> AppResult<()> {
    if semitones.unsigned_abs() > MAX_KEY_SHIFT {
        return Err(AppError::Other(format!("a key shift is ±{MAX_KEY_SHIFT} semitones at most")));
    }
    state.with_track(deck, |track| track.key_shift = semitones)?;
    state.send_if_running(Command::SetKeyShift { deck, semitones })
}

/// How the track on `deck` plays each part, until the next load: the parts it turns on or off
/// from the next bar if `on_bar` (see `Command::SetStemMix`).
pub fn set_stem_mix(state: &AppState, deck: usize, mix: StemMixDto, on_bar: bool) -> AppResult<()> {
    state.with_track(deck, |t| t.stem_mix = mix)?;
    state.send_if_running(Command::SetStemMix { deck, mix: mix.into(), on_bar })
}

/// How loud `part` of the track on `deck` plays, 0 to 1: nothing on a track without it.
pub fn set_stem_level(state: &AppState, deck: usize, part: StemPart, level: f32) -> AppResult<()> {
    let mix = |t: &mut LoadedTrack| t.separation().has(part).then(|| t.stem_mix.with_level(part, level));
    match state.with_track(deck, mix) {
        Ok(Some(mix)) => set_stem_mix(state, deck, mix, false),
        _ => Ok(()),
    }
}

/// The deck chosen to lead the others; `None`: the engine picks it.
pub fn set_sync_leader(state: &AppState, deck: Option<usize>) -> AppResult<()> {
    if let Some(d) = deck {
        state.has_deck(d)?;
    }
    *lock(&state.sync_leader) = deck;
    state.send(Command::SetSyncLeader { deck })
}

/// `deck` leads the others, or, leading, lets the engine pick the leader again.
pub fn toggle_leader(state: &AppState, deck: usize) -> AppResult<()> {
    let leads = state.deck_status(deck).is_ok_and(|status| status.sync.leader);
    set_sync_leader(state, (!leads).then_some(deck))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::bench::Bench;

    fn turned(deck: usize, action: DeckAction, steps: f32, at_s: f64, jog_scratch: bool) -> Option<Command> {
        let input = Input::of(steps, action != DeckAction::JogTouch, None);
        jog(deck, action, input, at_s, 720.0, jog_scratch)
    }

    #[test]
    fn a_jog_turns_by_its_steps_on_its_controllers_clock() {
        let turned_back = turned(1, DeckAction::Jog, -180.0, 2.5, true);
        assert!(
            matches!(turned_back, Some(Command::Jog { deck: 1, turns, at_s }) if turns == -0.25 && at_s == 2.5)
        );
        let touched = turned(0, DeckAction::JogTouch, 0.2, 3.0, true);
        assert!(matches!(touched, Some(Command::JogTouch { deck: 0, touched: true, .. })));
        let let_go = turned(0, DeckAction::JogTouch, 0.0, 3.5, true);
        assert!(matches!(let_go, Some(Command::JogTouch { deck: 0, touched: false, .. })));
        assert!(
            turned(0, DeckAction::JogTouch, 1.0, 4.0, false).is_none(),
            "a wheel that only nudges holds nothing"
        );
    }

    #[test]
    fn a_jog_searches_seconds_a_turn() {
        let searched = turned(0, DeckAction::JogSearch, 360.0, 0.0, true);
        assert!(matches!(searched, Some(Command::Jump { deck: 0, seconds }) if seconds == 5.0));
    }

    /// The ratio the tempo of deck A was set to by its fader moved to `value` by `device`.
    fn pitch(bench: &mut Bench, device: &str, value: f32) -> Option<f64> {
        bench.moved(device, "deck1.pitch", Input::Value(value)).unwrap();
        bench.sent().into_iter().find_map(|c| match c {
            Command::SetPitch { deck: 0, ratio } => Some(ratio),
            _ => None,
        })
    }

    #[test]
    fn a_tempo_fader_picks_the_tempo_up_before_moving_it() {
        let mut bench = Bench::new();
        // the screen left the tempo at +4 %: the fader at -8 % does not jump it there
        set_pitch(&bench.state, 0, 1.04).unwrap();
        bench.sent();
        assert_eq!(pitch(&mut bench, "DDJ", 0.0), None);
        assert_eq!(pitch(&mut bench, "DDJ", 0.5), None);
        let picked = pitch(&mut bench, "DDJ", 0.8).unwrap();
        assert!((picked - 1.048).abs() < 1e-6);
        assert!((pitch(&mut bench, "DDJ", 1.0).unwrap() - 1.08).abs() < 1e-6);
        assert!((lock(&bench.state.decks)[0].pitch - 1.08).abs() < 1e-6, "kept");
    }

    #[test]
    fn two_controllers_on_one_deck_each_pick_its_tempo_up_and_the_interface_sets_it() {
        let mut bench = Bench::new();
        assert!(pitch(&mut bench, "DDJ", 0.5).is_some());
        assert_eq!(pitch(&mut bench, "CDJ", 0.9), None, "its fader is elsewhere");
        assert!(pitch(&mut bench, "DDJ", 0.55).is_some());
        bench.perform("deck1.pitch", Input::Value(1.0), super::super::Source::Interface).unwrap();
        assert!((lock(&bench.state.decks)[0].pitch - 1.08).abs() < 1e-6);
    }

    #[test]
    fn a_deck_is_chosen_to_lead_the_others() {
        let mut bench = Bench::new();
        bench.press("deck2.sync.leader", 1.0).unwrap();
        bench.press("deck2.sync.leader", 0.0).unwrap();
        assert!(matches!(bench.sent()[..], [Command::SetSyncLeader { deck: Some(1) }]));
        assert_eq!(*lock(&bench.state.sync_leader), Some(1));
    }
}
