//! A deck's actions performed here: the jog wheels, played in time (a jog searching moves
//! through the track quickly, under shift say), and the tempo faders, moved finely.

use super::{Actions, Source};
use crate::auto_dj::{AutoDj, Takeover};
use crate::error::AppResult;
use crate::state::{lock, AppState};
use prisme_actions::input::{Given, Input};
use prisme_actions::scales::Tempo;
use prisme_control::{DeckAction, Target, JOG_STEPS_PER_TURN};
use prisme_engine::Command;
use tauri::{AppHandle, Manager};

/// Seconds of track a turn of a jog wheel goes through in a search.
const SEARCH_SECONDS_PER_TURN: f64 = 10.0;

/// Performs `action` of `deck` driven by `input`; nothing for a deck not in use.
pub(super) fn perform(
    app: &AppHandle,
    actions: &Actions,
    deck: usize,
    action: DeckAction,
    input: Input,
    source: Source,
) -> AppResult<()> {
    let state = app.state::<AppState>();
    if state.has_deck(deck).is_err() {
        return Ok(());
    }
    let range = state.tempo_range(deck);
    let settings = lock(&state.decks)[deck];
    let tempo = Tempo { ratio: settings.pitch, range };
    let Some(command) = command(actions, deck, action, input, source, tempo, settings.jog_scratch) else {
        return Ok(());
    };
    match command {
        Command::SetPitch { ratio, .. } => lock(&state.decks)[deck].pitch = ratio,
        _ => {
            if let Some(auto_dj) = app.try_state::<AutoDj>() {
                auto_dj.take_over(Takeover::Transport(deck));
            }
        }
    }
    state.send_if_running(command)
}

/// The command for `action` of `deck` driven by `input` as `source` asks, the deck at `tempo`
/// and its jog wheel scratching or not (`jog_scratch`). None for a wheel's touch on a deck
/// whose wheel only nudges, a fader not at its setting yet, an action not performed here.
fn command(
    actions: &Actions,
    deck: usize,
    action: DeckAction,
    input: Input,
    source: Source,
    tempo: Tempo,
    jog_scratch: bool,
) -> Option<Command> {
    let (at_s, steps_per_turn) = match source {
        Source::Controller(moved) => (moved.at_s, moved.jog_steps_per_turn),
        Source::Interface => (actions.now_s(), JOG_STEPS_PER_TURN),
    };
    let turns = f64::from(input.amount()) / steps_per_turn;
    match action {
        DeckAction::Jog => Some(Command::Jog { deck, turns, at_s }),
        // however lightly: some wheels tell how hard they are touched; a wheel that only
        // nudges: its top is no hand on the record
        DeckAction::JogTouch => {
            jog_scratch.then_some(Command::JogTouch { deck, touched: input.amount() > 0.0, at_s })
        }
        DeckAction::JogSearch => Some(Command::Jump { deck, seconds: turns * SEARCH_SECONDS_PER_TURN }),
        DeckAction::Pitch => {
            match actions.given(source, &Target::Deck(deck, action), input, Some(tempo.position()))? {
                Given::Value(position) => Some(Command::SetPitch { deck, ratio: tempo.at(position) }),
                Given::Steps(_) => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::Moved;

    const TEMPO: Tempo = Tempo { ratio: 1.0, range: 0.08 };

    fn moved(device: &str, at_s: f64) -> Source<'_> {
        Source::Controller(Moved { device, at_s, jog_steps_per_turn: 720.0 })
    }

    fn of(actions: &Actions, source: Source, deck: usize, action: DeckAction, value: f32) -> Option<Command> {
        let input = Input::of(value, action != DeckAction::Pitch, None);
        command(actions, deck, action, input, source, TEMPO, true)
    }

    #[test]
    fn a_jog_turns_by_its_steps_on_its_controllers_clock() {
        let actions = Actions::start();
        let turned = of(&actions, moved("DDJ", 2.5), 1, DeckAction::Jog, -180.0);
        assert!(
            matches!(turned, Some(Command::Jog { deck: 1, turns, at_s }) if turns == -0.25 && at_s == 2.5)
        );
        let touched = of(&actions, moved("DDJ", 3.0), 0, DeckAction::JogTouch, 0.2);
        assert!(matches!(touched, Some(Command::JogTouch { deck: 0, touched: true, .. })));
        let let_go = of(&actions, moved("DDJ", 3.5), 0, DeckAction::JogTouch, 0.0);
        assert!(matches!(let_go, Some(Command::JogTouch { deck: 0, touched: false, .. })));
        let nudging =
            command(&actions, 0, DeckAction::JogTouch, Input::Value(1.0), moved("DDJ", 4.0), TEMPO, false);
        assert!(nudging.is_none(), "a wheel that only nudges holds nothing");
    }

    #[test]
    fn a_jog_searches_seconds_a_turn() {
        let actions = Actions::start();
        let searched = of(&actions, moved("DDJ", 0.0), 0, DeckAction::JogSearch, 360.0);
        assert!(matches!(searched, Some(Command::Jump { deck: 0, seconds }) if seconds == 5.0));
    }

    fn pitch(actions: &Actions, source: Source, value: f32, ratio: f64) -> Option<f64> {
        let tempo = Tempo { ratio, ..TEMPO };
        match command(actions, 0, DeckAction::Pitch, Input::Value(value), source, tempo, true) {
            Some(Command::SetPitch { ratio, .. }) => Some(ratio),
            _ => None,
        }
    }

    #[test]
    fn a_tempo_fader_picks_the_tempo_up_before_moving_it() {
        let actions = Actions::start();
        // the screen left the tempo at +4 %: the fader at -8 % does not jump it there
        assert_eq!(pitch(&actions, moved("DDJ", 0.0), 0.0, 1.04), None);
        assert_eq!(pitch(&actions, moved("DDJ", 0.0), 0.5, 1.04), None);
        let picked = pitch(&actions, moved("DDJ", 0.0), 0.8, 1.04).unwrap();
        assert!((picked - 1.048).abs() < 1e-6);
        assert!((pitch(&actions, moved("DDJ", 0.0), 1.0, picked).unwrap() - 1.08).abs() < 1e-6);
    }

    #[test]
    fn two_controllers_on_one_deck_each_pick_its_tempo_up_and_the_interface_sets_it() {
        let actions = Actions::start();
        assert!(pitch(&actions, moved("DDJ", 0.0), 0.5, 1.0).is_some());
        assert_eq!(pitch(&actions, moved("CDJ", 0.0), 0.9, 1.0), None, "its fader is elsewhere");
        assert!(pitch(&actions, moved("DDJ", 0.0), 0.55, 1.0).is_some());
        assert!((pitch(&actions, Source::Interface, 1.0, 1.0).unwrap() - 1.08).abs() < 1e-6);
    }
}
