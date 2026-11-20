//! A deck's performance pads held: a roll, a slice of its slicer, a fader cut; of the slices or
//! the cuts held, the last pressed plays, and the one before once it is let go.

use super::{transport, Performing};
use crate::error::AppResult;
use crate::state::{lock, AppState};
use prisme_actions::pads::{bar_start, slice_at, CUT_BEATS};
use prisme_engine::{Command, LoopRegion};

/// Where the track on `deck` is (under a loop or a roll, where it would be), where the first
/// beat of its grid is, and how long a beat lasts: none without a track or a grid.
struct Place {
    at_s: f64,
    first_beat_s: f64,
    beat_s: f64,
}

fn place(state: &AppState, deck: usize) -> Option<Place> {
    let grid = state.with_track(deck, |t| t.grid()).ok().flatten()?;
    let status = state.deck_status(deck).ok().filter(|s| s.loaded)?;
    let at_s = status.slip_position_s.unwrap_or(status.position_s);
    Some(Place { at_s, first_beat_s: grid.first_beat_s, beat_s: grid.beat_period_s() })
}

impl Place {
    /// Where a slicer put here starts: the bar the track is in.
    fn bar_start(&self) -> f64 {
        bar_start(self.at_s, self.first_beat_s, self.beat_s)
    }
}

/// The slicer of `deck` put on the bar the track is in (as the screen's slicer is chosen).
pub fn anchor_slicer(state: &AppState, deck: usize) -> AppResult<()> {
    state.has_deck(deck)?;
    if let Some(place) = place(state, deck) {
        lock(&state.perform).decks[deck].slicer = Some(place.bar_start());
    }
    Ok(())
}

impl Performing<'_> {
    /// A roll of `beats` while `held`; let go, the track plays on where it would be.
    pub(super) fn roll(&self, deck: usize, beats: f64, held: bool) -> AppResult<()> {
        let beats = held.then_some(beats);
        let sent = transport::send(self.state, deck, Command::LoopRoll { deck, beats });
        self.perform_state().decks[deck].roll = beats.filter(|_| sent.is_ok());
        sent
    }

    /// Slice `slice` of the slicer played round and round while `held`, the track going on
    /// underneath. A slicer the track has left is put on the bar it is in at the first press.
    pub(super) fn slice(&self, deck: usize, slice: usize, held: bool) -> AppResult<()> {
        let place = place(self.state, deck);
        let region = {
            let mut perform = self.perform_state();
            let d = &mut perform.decks[deck];
            if let Some(place) = place.as_ref().filter(|_| held && d.slices.acting().is_none()) {
                let left = d.slicer.is_none_or(|start| slice_at(start, place.at_s, place.beat_s).is_none());
                if left {
                    d.slicer = Some(place.bar_start());
                }
            }
            let (before, now) = d.slices.press(slice, held);
            if before == now {
                return Ok(());
            }
            let slicer = d.slicer.zip(place.map(|p| p.beat_s));
            now.zip(slicer).map(|(slice, (start_s, beat_s))| {
                let start_s = start_s + slice as f64 * beat_s;
                LoopRegion { start_s, end_s: start_s + beat_s }
            })
        };
        transport::send(self.state, deck, Command::Slice { deck, region })
    }

    /// The deck cut on the beats of fader cut `cut` while `held`.
    pub(super) fn cut(&self, deck: usize, cut: usize, held: bool) -> AppResult<()> {
        let (before, now) = self.perform_state().decks[deck].cuts.press(cut, held);
        if before == now {
            return Ok(());
        }
        let beats = now.map(|cut| CUT_BEATS[cut]);
        transport::send(self.state, deck, Command::FaderCut { deck, beats })
    }
}

#[cfg(test)]
mod tests {
    use crate::actions::bench::Bench;
    use crate::state::lock;
    use prisme_engine::{Command, LoopRegion};

    fn hold(bench: &Bench, id: &str, held: bool) {
        bench.press(id, if held { 1.0 } else { 0.0 }).unwrap();
    }

    #[test]
    fn a_roll_plays_while_held_named_by_its_place_or_its_length() {
        let bench = Bench::new();
        hold(&bench, "deck1.roll4", true);
        assert_eq!(lock(&bench.state.perform).decks[0].roll, Some(0.5));
        hold(&bench, "deck1.roll4", false);
        hold(&bench, "deck2.roll.2", true);
        assert_eq!(lock(&bench.state.perform).decks[1].roll, Some(2.0));
        hold(&bench, "deck2.roll.2", false);
        assert_eq!(lock(&bench.state.perform).decks[1].roll, None);
        assert!(matches!(
            bench.sent()[..],
            [
                Command::LoopRoll { deck: 0, beats: Some(0.5) },
                Command::LoopRoll { deck: 0, beats: None },
                Command::LoopRoll { deck: 1, beats: Some(2.0) },
                Command::LoopRoll { deck: 1, beats: None },
            ]
        ));
    }

    /// The slices the engine was told to play since last asked, by their start.
    fn slices(bench: &mut Bench) -> Vec<Option<f64>> {
        bench
            .sent()
            .into_iter()
            .filter_map(|c| match c {
                Command::Slice { region, .. } => Some(region.map(|r: LoopRegion| r.start_s)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn two_slices_held_the_last_plays_and_letting_it_go_plays_the_first() {
        let mut bench = Bench::new();
        // 120 BPM: a beat of half a second, a bar of two
        bench.load(0, 60.0, Some(120.0));
        bench.at(0, 5.3);
        hold(&bench, "deck1.slice2", true);
        assert_eq!(lock(&bench.state.perform).decks[0].slicer, Some(4.0), "on the bar the track is in");
        hold(&bench, "deck1.slice5", true);
        hold(&bench, "deck1.slice5", false);
        hold(&bench, "deck1.slice2", false);
        assert_eq!(slices(&mut bench), [Some(4.5), Some(6.0), Some(4.5), None]);
    }

    #[test]
    fn the_slicer_stays_while_the_track_is_in_it_and_follows_it_once_it_left() {
        let bench = Bench::new();
        bench.load(0, 60.0, Some(120.0));
        super::anchor_slicer(&bench.state, 0).unwrap();
        assert_eq!(lock(&bench.state.perform).decks[0].slicer, Some(0.0));
        bench.at(0, 3.0);
        hold(&bench, "deck1.slice1", true);
        hold(&bench, "deck1.slice1", false);
        assert_eq!(lock(&bench.state.perform).decks[0].slicer, Some(0.0), "within its eight beats");
        bench.at(0, 9.0);
        hold(&bench, "deck1.slice1", true);
        assert_eq!(lock(&bench.state.perform).decks[0].slicer, Some(8.0));
    }

    #[test]
    fn what_the_pads_hold_reaches_the_interface() {
        let bench = Bench::new();
        let before = bench.state.revision();
        hold(&bench, "deck2.cut2", true);
        hold(&bench, "deck2.roll.1", true);
        assert!(bench.state.revision() > before);
        let held = &crate::dto::ControlStateDto::of(&bench.state).perform[1];
        assert_eq!((held.cut, held.roll, held.slicer), (Some(1), Some(1.0), None));
    }

    #[test]
    fn two_cuts_held_the_last_cuts_and_letting_it_go_the_first_cuts_again() {
        let bench = Bench::new();
        hold(&bench, "deck2.cut1", true);
        hold(&bench, "deck2.cut3", true);
        assert_eq!(lock(&bench.state.perform).decks[1].cuts.acting(), Some(2));
        hold(&bench, "deck2.cut3", false);
        hold(&bench, "deck2.cut1", false);
        let cuts: Vec<Option<f64>> = bench
            .sent()
            .into_iter()
            .filter_map(|c| match c {
                Command::FaderCut { deck: 1, beats } => Some(beats),
                _ => None,
            })
            .collect();
        assert_eq!(cuts, [Some(0.0625), Some(0.25), Some(0.0625), None]);
    }
}
