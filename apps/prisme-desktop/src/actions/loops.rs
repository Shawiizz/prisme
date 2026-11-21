//! A deck's loops: set and left by its keys, the loop stored played again from its start with
//! the deck stopped, its edges gone to, moved by beats of the track's grid.

use super::{transport, Performing};
use crate::error::AppResult;
use crate::state::lock;
use prisme_actions::pads::plays_loop;
use prisme_control::{Beats, DeckAction};
use prisme_engine::{Command, LoopStatus};
use transport::LOOP_KEY_BEATS;

impl Performing<'_> {
    /// A loop key of `deck` pressed; none for the other keys.
    pub(super) fn loop_key(&self, deck: usize, action: DeckAction) -> Option<AppResult<()>> {
        let state = self.state;
        let send = |command| transport::send(state, deck, command);
        Some(match action {
            DeckAction::LoopIn => send(Command::LoopIn { deck }),
            DeckAction::LoopOut => send(Command::LoopOut { deck }),
            DeckAction::Reloop => send(Command::Reloop { deck }),
            DeckAction::LoopHalve => send(Command::ScaleLoop { deck, factor: 0.5 }),
            DeckAction::LoopDouble => send(Command::ScaleLoop { deck, factor: 2.0 }),
            DeckAction::LoopBeats(Beats(beats)) => self.loop_of(deck, beats),
            DeckAction::Loop => self.toggle_loop(deck, LOOP_KEY_BEATS),
            DeckAction::LoopSize => self.loop_of(deck, lock(&state.decks)[deck].size_beats),
            DeckAction::LoopClear => transport::set_loop(state, deck, None, None, false),
            DeckAction::ReloopStop => self.reloop_stopped(deck),
            DeckAction::LoopInGoto => self.to_loop_edge(deck, false),
            DeckAction::LoopOutGoto => self.to_loop_edge(deck, true),
            DeckAction::LoopMoveBack => self.move_loop(deck, -1),
            DeckAction::LoopMoveForward => self.move_loop(deck, 1),
            _ => return None,
        })
    }

    /// A loop of `beats` where `deck` plays, or out of the loop it plays.
    fn toggle_loop(&self, deck: usize, beats: f64) -> AppResult<()> {
        let looping = self.state.deck_status(deck).is_ok_and(|status| status.looping.active);
        let command = if looping { Command::ExitLoop { deck } } else { Command::BeatLoop { deck, beats } };
        transport::send(self.state, deck, command)
    }

    /// A loop of `beats` where `deck` plays, or out of it if it plays that loop.
    fn loop_of(&self, deck: usize, beats: f64) -> AppResult<()> {
        let status = self.state.deck_status(deck)?;
        let command = if plays_loop(&status.looping, beats) {
            Command::ExitLoop { deck }
        } else {
            Command::BeatLoop { deck, beats }
        };
        transport::send(self.state, deck, command)
    }

    /// The loop of `deck`, if one is stored.
    fn stored_loop(&self, deck: usize) -> Option<LoopStatus> {
        let looping = self.state.deck_status(deck).ok()?.looping;
        looping.region.is_some().then_some(looping)
    }

    /// Back to the start of the stored loop, playing it, the deck stopped.
    pub(super) fn reloop_stopped(&self, deck: usize) -> AppResult<()> {
        let Some(region) = self.stored_loop(deck).and_then(|l| l.region) else { return Ok(()) };
        transport::set_loop(self.state, deck, Some(region.start_s), Some(region.end_s), true)?;
        self.to(deck, region.start_s, Some(false))
    }

    /// To the start of the stored loop, or to its end.
    pub(super) fn to_loop_edge(&self, deck: usize, end: bool) -> AppResult<()> {
        let Some(region) = self.stored_loop(deck).and_then(|l| l.region) else { return Ok(()) };
        self.seek(deck, if end { region.end_s } else { region.start_s })
    }

    /// The stored loop moved by `beats` of the track's grid (back below zero), playing or not
    /// as it was: nothing without a grid.
    pub(super) fn move_loop(&self, deck: usize, beats: i32) -> AppResult<()> {
        let Some(looping) = self.stored_loop(deck) else { return Ok(()) };
        let grid = self.state.with_track(deck, |t| t.grid()).ok().flatten();
        let (Some(region), Some(grid)) = (looping.region, grid) else { return Ok(()) };
        let by = f64::from(beats) * grid.beat_period_s();
        transport::set_loop(
            self.state,
            deck,
            Some(region.start_s + by),
            Some(region.end_s + by),
            looping.active,
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::actions::bench::Bench;
    use prisme_actions::input::Input;
    use prisme_engine::LoopRegion;

    fn press(bench: &Bench, id: &str) {
        bench.press(id, 1.0).unwrap();
        bench.press(id, 0.0).unwrap();
        bench.run(0.01);
    }

    fn region(bench: &Bench, deck: usize) -> Option<(f64, f64)> {
        bench.status(deck).looping.region.map(|r: LoopRegion| (r.start_s, r.end_s))
    }

    /// A deck at 120 BPM playing a loop of four beats from the start, its tempo fader at +10 %.
    fn looping() -> Bench {
        let bench = Bench::new();
        bench.load(0, 60.0, Some(120.0));
        crate::actions::deck::set_pitch(&bench.state, 0, 1.1).unwrap();
        press(&bench, "deck1.loop.4");
        press(&bench, "deck1.play");
        bench.run(0.5);
        bench
    }

    #[test]
    fn a_loop_moves_by_the_beats_of_the_tracks_grid_whatever_its_tempo() {
        let bench = looping();
        press(&bench, "deck1.loop.move.forward");
        assert_eq!(region(&bench, 0), Some((0.5, 2.5)));
        bench.moved("DDJ", "deck1.loop.move", Input::Steps { steps: -3.0, step: None }).unwrap();
        bench.run(0.01);
        assert_eq!(region(&bench, 0), Some((-1.0, 1.0)));
        press(&bench, "deck1.loop.move.back");
        assert!(bench.status(0).looping.active, "playing as it was");
    }

    #[test]
    fn the_loop_played_again_stopped_and_its_edges_gone_to() {
        let bench = looping();
        press(&bench, "deck1.loop");
        assert!(!bench.status(0).looping.active);
        press(&bench, "deck1.loop.out.goto");
        assert!((bench.status(0).position_s - 2.0).abs() < 0.05, "playing on from there");
        press(&bench, "deck1.reloop.stop");
        let status = bench.status(0);
        assert!(status.looping.active && !status.playing);
        assert_eq!(status.position_s, 0.0);
        bench.at(0, 10.0);
        press(&bench, "deck1.loop.in.goto");
        assert_eq!(bench.status(0).position_s, 0.0);
    }

    #[test]
    fn the_loop_keys_of_a_deck_with_no_loop_do_nothing() {
        let bench = Bench::new();
        bench.load(1, 60.0, Some(120.0));
        for id in ["deck2.reloop.stop", "deck2.loop.in.goto", "deck2.loop.move.forward"] {
            press(&bench, id);
        }
        assert!(bench.sent().is_empty());
        assert_eq!(region(&bench, 1), None);
    }
}
