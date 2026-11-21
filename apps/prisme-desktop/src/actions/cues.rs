//! A deck's cues, kept in the library: its main cue point, which a CDJ's cue key plays from,
//! and its hot cues. A cue changed changes at once on every deck playing the file, then is kept
//! in the library in turn (see `Actions::keep`); the interface hears of it with the decks'
//! tracks.

use super::Performing;
use crate::auto_dj::Takeover;
use crate::error::AppResult;
use crate::state::{lock, AppState, LoadedTrack};
use prisme_actions::cue::{self, CuePress};
use prisme_actions::screen::Drive;
use prisme_engine::{BeatGrid, Command, LoopRegion};
use prisme_library::{Cue, CueSlot};

impl Performing<'_> {
    /// The cue key of `deck` pressed (`held`) or let go (see `prisme_actions::cue`): let go
    /// while it plays from the cue point, back there.
    pub(super) fn cue(&self, deck: usize, held: bool) -> AppResult<()> {
        if !held {
            return if self.take_cue_preview(deck) { self.to_cue(deck, Some(false)) } else { Ok(()) };
        }
        let Ok(status) = self.state.deck_status(deck) else { return Ok(()) };
        match cue::pressed(&status, main_cue_s(self.state, deck)) {
            None => Ok(()),
            Some(CuePress::Back) => self.to_cue(deck, Some(false)),
            Some(CuePress::Preview) => {
                self.memory().decks[deck].cue_preview = true;
                self.set_playing(deck, true)
            }
            Some(CuePress::Set(at_s)) => {
                let cue_s = self.set_main_cue(deck, at_s)?;
                self.seek(deck, cue_s)
            }
        }
    }

    /// PLAY: the deck plays or pauses; pressed while the cue key plays from the cue point, the
    /// deck plays on once the cue key is let go, as on a CDJ. A track just put on the deck
    /// plays, whether the engine has taken it yet or not.
    pub(super) fn play(&self, deck: usize) -> AppResult<()> {
        if self.take_cue_preview(deck) || lock(&self.state.tracks)[deck].is_none() {
            return Ok(());
        }
        match self.state.deck_status(deck) {
            Ok(status) => self.set_playing(deck, !status.playing),
            Err(_) => Ok(()),
        }
    }

    /// Whether the cue key of `deck` played from the cue point, which it no longer does.
    fn take_cue_preview(&self, deck: usize) -> bool {
        std::mem::take(&mut self.memory().decks[deck].cue_preview)
    }

    /// To the main cue point of `deck` (the start without one), then playing, stopped or as it
    /// was (`then`).
    pub(super) fn to_cue(&self, deck: usize, then: Option<bool>) -> AppResult<()> {
        self.to(deck, main_cue_s(self.state, deck), then)
    }

    /// To `seconds` into the track on `deck`, then playing, stopped or as it was (`then`).
    pub(super) fn to(&self, deck: usize, seconds: f64, then: Option<bool>) -> AppResult<()> {
        self.seek(deck, seconds)?;
        then.map_or(Ok(()), |playing| self.set_playing(deck, playing))
    }

    /// The main cue point where `deck` plays (on the nearest beat with quantize). A stopped
    /// deck goes there, on the cue point then; a playing one plays on, with no jump in the
    /// music, and a record holding the deck in place keeps it where it is.
    pub(super) fn set_cue_here(&self, deck: usize) -> AppResult<()> {
        let Ok(status) = self.state.deck_status(deck) else { return Ok(()) };
        if !status.loaded {
            return Ok(());
        }
        let cue_s = self.set_main_cue(deck, status.position_s)?;
        if cue::playing(&status) || self.state.drive(deck) == Drive::Absolute {
            Ok(())
        } else {
            self.seek(deck, cue_s)
        }
    }

    /// Stores the main cue point of `deck` at `seconds` (on the nearest beat with quantize):
    /// where it went.
    fn set_main_cue(&self, deck: usize, seconds: f64) -> AppResult<f64> {
        let quantize = self.state.preferences.read(|p| p.dj.quantize);
        let cue = self
            .state
            .with_track(deck, |track| Cue::point(CueSlot::Main, snapped(track.grid(), seconds, quantize)))?;
        let position_s = cue.position_s;
        self.put_cue(deck, cue)?;
        Ok(position_s)
    }

    /// Hot cue `slot` of `deck` pressed: the track jumps there, a saved loop playing; an empty
    /// slot is set where the track is (the loop playing, if one does). The Auto DJ lets the
    /// deck go.
    pub(super) fn hot_cue(&self, deck: usize, slot: usize) -> AppResult<()> {
        let Ok(cue) = self.state.with_track(deck, |t| t.cue(hot(slot)).cloned()) else { return Ok(()) };
        self.take_over(Takeover::Transport(deck));
        match cue {
            Some(cue) => {
                if let Some(length_s) = cue.loop_length_s {
                    let region = LoopRegion { start_s: cue.position_s, end_s: cue.position_s + length_s };
                    self.state.send(Command::SetLoop { deck, region: Some(region), active: true })?;
                }
                self.state.send(Command::Jump { deck, seconds: cue.position_s })
            }
            None => self.set_hot_cue(deck, slot),
        }
    }

    /// Sets hot cue `slot` of `deck` where the track is (on the nearest beat with quantize), or
    /// as a saved loop if a loop plays.
    fn set_hot_cue(&self, deck: usize, slot: usize) -> AppResult<()> {
        let status = self.state.deck_status(deck)?;
        let quantize = self.state.preferences.read(|p| p.dj.quantize);
        let cue = self.state.with_track(deck, |track| {
            match status.looping.region.filter(|_| status.looping.active) {
                Some(region) => Cue::looped(hot(slot), region.start_s, region.length_s()),
                None => Cue::point(hot(slot), snapped(track.grid(), status.position_s, quantize)),
            }
        })?;
        self.put_cue(deck, cue)
    }

    /// Takes hot cue `slot` off the track on `deck`.
    pub(super) fn delete_hot_cue(&self, deck: usize, slot: usize) -> AppResult<()> {
        let slot = hot(slot);
        let path = self.state.with_track(deck, |track| track.path.clone())?;
        self.state.change_file(&path, |track| track.remove_cue(slot));
        self.keep(move |library| library.delete_cue(&path, slot));
        Ok(())
    }

    /// `cue` for the track on `deck`, on every deck playing it, then in the library.
    fn put_cue(&self, deck: usize, cue: Cue) -> AppResult<()> {
        let path = self.state.with_track(deck, |track| track.path.clone())?;
        self.state.change_file(&path, |track: &mut LoadedTrack| track.put_cue(cue.clone()));
        self.keep(move |library| library.save_cue(&path, &cue));
        Ok(())
    }
}

/// Where the main cue point of `deck` is: the start of its track without one.
fn main_cue_s(state: &AppState, deck: usize) -> f64 {
    let cue = state.with_track(deck, |t| t.cue(CueSlot::Main).map(|c| c.position_s));
    cue.ok().flatten().unwrap_or(0.0)
}

/// Hot cue `slot` of an action: one of the 16.
fn hot(slot: usize) -> CueSlot {
    CueSlot::Hot(slot as u8)
}

fn snapped(grid: Option<BeatGrid>, position_s: f64, quantize: bool) -> f64 {
    match grid {
        Some(grid) if quantize => grid.nearest_beat_s(position_s),
        _ => position_s,
    }
}

#[cfg(test)]
mod tests {
    use crate::actions::bench::Bench;
    use crate::state::lock;
    use prisme_library::CueSlot;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.05
    }

    /// `id` pressed then let go, the engine run after each.
    fn press(bench: &Bench, id: &str) {
        bench.press(id, 1.0).unwrap();
        bench.run(0.01);
        bench.press(id, 0.0).unwrap();
        bench.run(0.01);
    }

    fn main_cue(bench: &Bench, deck: usize) -> Option<f64> {
        bench.state.with_track(deck, |t| t.cue(CueSlot::Main).map(|c| c.position_s)).unwrap()
    }

    #[test]
    fn the_cue_held_on_the_cue_point_plays_until_let_go_then_goes_back_there() {
        let bench = Bench::new();
        bench.load(0, 60.0, None);
        bench.press("deck1.cue", 1.0).unwrap();
        bench.run(0.5);
        assert!(bench.status(0).playing);
        bench.press("deck1.cue", 0.0).unwrap();
        bench.run(0.01);
        let status = bench.status(0);
        assert!(!status.playing);
        assert_eq!(status.position_s, 0.0);
    }

    #[test]
    fn the_cue_held_on_the_cue_point_and_play_pressed_plays_on_once_the_cue_is_let_go() {
        let bench = Bench::new();
        bench.load(0, 60.0, None);
        bench.press("deck1.cue", 1.0).unwrap();
        bench.run(0.2);
        press(&bench, "deck1.play");
        bench.press("deck1.cue", 0.0).unwrap();
        bench.run(0.2);
        let status = bench.status(0);
        assert!(status.playing && status.position_s > 0.3);
        press(&bench, "deck1.play");
        assert!(!bench.status(0).playing, "PLAY pauses it then");
    }

    #[test]
    fn the_cue_pressed_playing_goes_back_to_the_cue_point_stopped_elsewhere_sets_it_there() {
        let bench = Bench::new();
        bench.load(0, 60.0, Some(120.0));
        press(&bench, "deck1.play");
        bench.run(1.2);
        press(&bench, "deck1.cue");
        let status = bench.status(0);
        assert!(!status.playing && status.position_s == 0.0);
        bench.state.update_preferences(|p| p.dj.quantize = true);
        bench.at(0, 1.3);
        press(&bench, "deck1.cue");
        assert_eq!(main_cue(&bench, 0), Some(1.5), "on the nearest beat");
        assert_eq!(bench.status(0).position_s, 1.5);
        bench.press("deck1.cue", 1.0).unwrap();
        bench.run(0.1);
        assert!(bench.status(0).playing, "on it, it plays while held");
    }

    #[test]
    fn the_cue_set_while_playing_plays_on_and_stopped_puts_the_track_on_it() {
        let bench = Bench::new();
        bench.load(0, 60.0, Some(120.0));
        bench.state.update_preferences(|p| p.dj.quantize = true);
        press(&bench, "deck1.play");
        bench.run(1.2);
        press(&bench, "deck1.cue.set");
        let status = bench.status(0);
        assert_eq!(main_cue(&bench, 0), Some(1.0));
        assert!(status.playing && status.position_s > 1.2, "no jump in the music");
        press(&bench, "deck1.play");
        bench.at(0, 2.2);
        press(&bench, "deck1.cue.set");
        assert_eq!(main_cue(&bench, 0), Some(2.0));
        assert_eq!(bench.status(0).position_s, 2.0);
    }

    #[test]
    fn the_keys_to_the_cue_the_start_and_the_end() {
        let bench = Bench::new();
        bench.load(1, 60.0, None);
        lock(&bench.state.tracks)[1]
            .as_mut()
            .unwrap()
            .put_cue(prisme_library::Cue::point(CueSlot::Main, 10.0));
        press(&bench, "deck2.cue.play");
        assert!(bench.status(1).playing && close(bench.status(1).position_s, 10.0));
        press(&bench, "deck2.cue.goto");
        assert!(bench.status(1).playing, "as it was");
        press(&bench, "deck2.cue.stop");
        assert!(!bench.status(1).playing && bench.status(1).position_s == 10.0);
        press(&bench, "deck2.start.play");
        assert!(bench.status(1).playing && close(bench.status(1).position_s, 0.0));
        press(&bench, "deck2.start.keep");
        assert!(bench.status(1).playing);
        press(&bench, "deck2.start");
        assert!(!bench.status(1).playing && bench.status(1).position_s == 0.0);
        press(&bench, "deck2.end");
        assert_eq!(bench.status(1).position_s, 60.0);
    }

    #[test]
    fn a_hot_cue_on_an_empty_slot_is_set_and_the_next_press_jumps_there() {
        let bench = Bench::new();
        bench.load(0, 60.0, None);
        bench.at(0, 30.0);
        press(&bench, "deck1.hotcue3");
        let set = bench.state.with_track(0, |t| t.cue(CueSlot::Hot(2)).map(|c| c.position_s)).unwrap();
        assert_eq!(set, Some(30.0));
        bench.actions.settle();
        assert_eq!(bench.library.cues(std::path::Path::new("deck0.flac")).unwrap().len(), 1, "kept");
        bench.at(0, 0.0);
        press(&bench, "deck1.hotcue3");
        assert_eq!(bench.status(0).position_s, 30.0);
        press(&bench, "deck1.hotcue3.delete");
        assert!(bench.state.with_track(0, |t| t.cues.is_empty()).unwrap());
        bench.actions.settle();
        assert!(bench.library.cues(std::path::Path::new("deck0.flac")).unwrap().is_empty());
    }

    #[test]
    fn a_hot_cue_set_while_a_loop_plays_keeps_it_and_plays_it_again() {
        let bench = Bench::new();
        bench.load(0, 60.0, Some(120.0));
        press(&bench, "deck1.loop.4");
        press(&bench, "deck1.play");
        bench.run(0.5);
        press(&bench, "deck1.hotcue1");
        let cue = bench.state.with_track(0, |t| t.cue(CueSlot::Hot(0)).cloned()).unwrap().unwrap();
        assert_eq!((cue.position_s, cue.loop_length_s), (0.0, Some(2.0)));
        press(&bench, "deck1.loop");
        assert!(!bench.status(0).looping.active);
        press(&bench, "deck1.hotcue1");
        assert!(bench.status(0).looping.active);
    }

    #[test]
    fn a_hot_cue_changes_on_every_deck_playing_the_file() {
        let bench = Bench::new();
        bench.load(0, 60.0, None);
        let double = lock(&bench.state.tracks)[0].as_ref().unwrap().double();
        lock(&bench.state.tracks)[1] = Some(double);
        press(&bench, "deck1.hotcue2");
        assert!(bench.state.with_track(1, |t| t.cue(CueSlot::Hot(1)).is_some()).unwrap());
        press(&bench, "deck2.hotcue2.delete");
        assert!(bench.state.with_track(0, |t| t.cues.is_empty()).unwrap());
    }

    #[test]
    fn the_cue_keys_of_a_deck_with_no_track_do_nothing() {
        let bench = Bench::new();
        for id in ["deck1.cue", "deck1.play", "deck1.cue.set", "deck1.hotcue1", "deck1.end"] {
            press(&bench, id);
        }
        assert!(!bench.status(0).playing);
    }
}
