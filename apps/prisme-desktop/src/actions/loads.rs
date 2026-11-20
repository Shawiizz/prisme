//! Tracks put on the decks and taken off, sounds put in the samplers, a track previewed in the
//! headphones: what is loaded or previewed is the track selected in the library as the action
//! was sent, or the one the interface named ([`load_track`]). The track of a deck that plays
//! leaves it once the DJ says so, or never if the DJ chose so (see [`Performing::may_replace`]).

use super::{deck, sampler, take_over, Actions, Answer, Order, Performing, Source};
use crate::auto_dj::Takeover;
use crate::error::{AppError, AppResult};
use crate::preview::Previews;
use crate::state::lock;
use crate::track_loading;
use prisme_actions::input::Given;
use prisme_actions::view::plays;
use prisme_control::{DeckAction, LibraryAction, Target};
use prisme_engine::Command;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// How far a step of an encoder moves the preview (s).
const PREVIEW_STEP_S: f64 = 4.0;

/// Loads the track at `path` on `deck`, or on the first deck without a track (deck A if none):
/// a row of the library, a track dropped, one the command palette found; `confirmed`, the DJ said
/// yes to what it replaces. Sent in turn with the interface's actions (`order`).
#[tauri::command]
pub async fn load_track(
    app: AppHandle,
    path: String,
    deck: Option<usize>,
    confirmed: bool,
    order: Order,
) -> AppResult<()> {
    crate::commands::blocking(move || {
        let actions = app.state::<Actions>();
        let (answer, outcome) = std::sync::mpsc::sync_channel(1);
        actions.in_order(order, || {
            actions.load_track(&app, PathBuf::from(path), deck, confirmed, Answer::To(answer));
        });
        outcome.recv().unwrap_or_else(|e| Err(AppError::Other(e.to_string())))
    })
    .await
}

impl Performing<'_> {
    /// A load of the library pressed: the selected track on a deck, played there or not, or on
    /// the first deck without a track (deck A if none), or the track of the deck beside it.
    pub(super) fn load(&self, action: LibraryAction) -> AppResult<()> {
        let target = Target::Library(action);
        match action {
            LibraryAction::Load(deck) => self.load_selected(&target, deck, false),
            LibraryAction::LoadPlay(deck) => self.load_selected(&target, deck, true),
            LibraryAction::LoadFree => self.load_selected(&target, self.actions.free_deck(self.state), false),
            LibraryAction::Double(deck) => self.double(&target, deck),
            LibraryAction::Preview => self.toggle_preview(),
            LibraryAction::PreviewStop => match self.app {
                Some(app) => app.state::<Previews>().stop(self.state),
                None => Ok(()),
            },
            browsing => self.browse(browsing, Given::Value(1.0)),
        }
    }

    /// The selected track previewed in the headphones, or stopped if it is the one previewed.
    fn toggle_preview(&self) -> AppResult<()> {
        let (Some(app), Some(path)) = (self.app, self.track()) else { return Ok(()) };
        app.state::<Previews>().toggle(app, path.to_owned())
    }

    /// The preview moved by an encoder's `steps`, a few seconds each, back below zero.
    pub(super) fn skip_preview(&self, steps: i32) -> AppResult<()> {
        let seconds = f64::from(steps) * PREVIEW_STEP_S;
        self.state.send_if_running(Command::SkipPreview { seconds })
    }

    /// The track taken off `deck`.
    pub(super) fn eject(&self, deck: usize) -> AppResult<()> {
        self.may_replace(&Target::Deck(deck, DeckAction::Eject), deck, true)?;
        let turn = self.state.ask_load(deck);
        self.state.put_track(&turn, None, [Command::Eject { deck }]).map(drop)
    }

    /// The sound of the selected track put in `slot` of `deck`'s sampler.
    pub(super) fn load_sample(&self, deck: usize, slot: usize) -> AppResult<()> {
        match self.track() {
            Some(path) => sampler::load_sample(self.state, deck, slot, path.to_owned()),
            None => Ok(()),
        }
    }

    /// A pad of the sampler: its sound stopped while it plays, played while it waits. On an
    /// empty slot it is sent as a load (`Actions::sent_as`): one emptied since does nothing,
    /// never opening a file on the thread that sent it.
    pub(super) fn smart_sample(&self, deck: usize, slot: usize) -> AppResult<()> {
        let state = self.state;
        if state.deck_status(deck).is_ok_and(|s| s.samples_playing & 1 << slot != 0) {
            sampler::stop_sample(state, deck, slot)
        } else if lock(&state.samplers)[deck][slot].is_some() {
            sampler::trigger_sample(state, deck, slot, None)
        } else {
            Ok(())
        }
    }

    /// The selected track put on `deck`, played once there if `play`; the Auto DJ lets the deck
    /// go. Nothing before the library is open.
    fn load_selected(&self, target: &Target, deck: usize, play: bool) -> AppResult<()> {
        let (Some(path), Some(library)) = (self.track(), self.library) else { return Ok(()) };
        let path = path.to_owned();
        self.may_replace(target, deck, false)?;
        take_over(self.auto_dj().as_deref(), Takeover::Load(deck));
        let previews = self.app.map(|app| app.state::<Previews>());
        let Some(completion) =
            track_loading::put_on_deck(self.state, library, previews.as_deref(), deck, path)?
        else {
            return Ok(());
        };
        if let Some(app) = self.app {
            completion.run_aside(app);
        }
        if play {
            self.set_playing(deck, true)?;
        }
        Ok(())
    }

    /// The track of the deck beside `deck` put on it where it is, playing if it plays (an
    /// instant double): nothing is read again. Nothing if that deck has no track.
    fn double(&self, target: &Target, deck: usize) -> AppResult<()> {
        let from = deck ^ 1;
        if self.state.with_track(from, |_| ()).is_err() {
            return Ok(());
        }
        self.may_replace(target, deck, false)?;
        take_over(self.auto_dj().as_deref(), Takeover::Load(deck));
        let turn = self.state.ask_load(deck);
        let track = self.state.with_track(from, |t| t.double())?;
        let auto_gain = self.state.preferences.read(|p| p.dj.auto_gain);
        let commands =
            track.load_commands(deck, auto_gain).into_iter().chain([Command::TakePlace { deck, from }]);
        if self.state.put_track(&turn, Some(track), commands)? {
            deck::loaded(self.state, deck);
        }
        Ok(())
    }

    /// Whether the track of `deck` may leave it, for another or for none (`eject`), as `target`
    /// asks: if the DJ does not hear it, or said yes to it; never if the DJ chose to keep what
    /// plays, else once the DJ is asked.
    fn may_replace(&self, target: &Target, deck: usize, eject: bool) -> AppResult<()> {
        let heard = self.state.deck_status(deck).is_ok_and(|s| plays(&s));
        if !heard || matches!(self.source(), Source::Interface { confirmed: true }) {
            Ok(())
        } else if self.state.preferences.read(|p| p.dj.protect_playing) {
            Err(AppError::DeckPlays { deck, eject })
        } else {
            Err(AppError::ConfirmReplace { target: target.to_string(), deck, eject })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::bench::Bench;
    use crate::state::{AppState, LoadedTrack};
    use prisme_actions::input::Input;

    fn confirmed(bench: &Bench, id: &str) -> AppResult<()> {
        bench.perform(id, Input::Value(1.0), Source::Interface { confirmed: true })
    }

    fn loaded(state: &AppState) -> [bool; 2] {
        let tracks = lock(&state.tracks);
        [tracks[0].is_some(), tracks[1].is_some()]
    }

    fn select(bench: &Bench, path: Option<&str>) {
        match path {
            Some(path) => bench.select(std::path::Path::new(path)),
            None => bench.unselect(),
        }
    }

    /// A track of a minute playing on deck A.
    fn playing(bench: &mut Bench) {
        bench.load(0, 60.0, Some(120.0));
        bench.press("deck1.play", 1.0).unwrap();
        bench.run(0.05);
    }

    #[test]
    fn the_track_of_a_deck_that_plays_leaves_it_once_the_dj_says_so() {
        let mut bench = Bench::new();
        playing(&mut bench);
        let asked = bench.press("deck1.eject", 1.0);
        assert!(
            matches!(&asked, Err(AppError::ConfirmReplace { target, deck: 0, eject: true }) if target == "deck1.eject"),
            "{asked:?}"
        );
        assert_eq!(loaded(&bench.state), [true, false], "asked first");
        confirmed(&bench, "deck1.eject").unwrap();
        assert_eq!(loaded(&bench.state), [false, false]);
        assert!(bench.sent().iter().any(|c| matches!(c, Command::Eject { deck: 0 })));
    }

    #[test]
    fn the_track_of_a_deck_that_plays_stays_there_if_the_dj_chose_so() {
        let mut bench = Bench::new();
        bench.state.update_preferences(|p| p.dj.protect_playing = true);
        playing(&mut bench);
        bench.load(1, 60.0, None);
        select(&bench, Some("next.flac"));
        assert!(matches!(
            bench.press("library.load1", 1.0),
            Err(AppError::DeckPlays { deck: 0, eject: false })
        ));
        assert!(matches!(bench.press("deck1.eject", 1.0), Err(AppError::DeckPlays { deck: 0, eject: true })));
        assert!(matches!(bench.press("library.double1", 1.0), Err(AppError::DeckPlays { deck: 0, .. })));
        assert_eq!(loaded(&bench.state), [true, true]);
        bench.press("library.double2", 1.0).unwrap();
    }

    #[test]
    fn a_stopped_deck_gives_its_track_up_without_asking() {
        let bench = Bench::new();
        bench.load(0, 60.0, None);
        bench.press("deck1.eject", 1.0).unwrap();
        bench.press("deck1.eject", 0.0).unwrap();
        assert_eq!(loaded(&bench.state), [false, false]);
        assert!(!bench.state.loading()[0], "the eject is over");
    }

    #[test]
    fn nothing_is_loaded_without_a_track_selected_nor_on_a_deck_not_in_use() {
        let mut bench = Bench::new();
        playing(&mut bench);
        select(&bench, None);
        for id in ["library.load1", "library.load1.play", "library.load", "deck1.sample1.load"] {
            bench.press(id, 1.0).unwrap();
        }
        select(&bench, Some("next.flac"));
        bench.press("library.load3", 1.0).unwrap();
        bench.press("library.double4", 1.0).unwrap();
        assert_eq!(loaded(&bench.state), [true, false]);
    }

    #[test]
    fn a_load_without_a_deck_goes_to_the_first_without_a_track() {
        let bench = Bench::new();
        let free = |bench: &Bench| bench.actions.free_deck(&bench.state);
        assert_eq!(free(&bench), 0);
        bench.load(0, 60.0, None);
        assert_eq!(free(&bench), 1);
        bench.load(1, 60.0, None);
        assert_eq!(free(&bench), 0, "deck A when they all have one");
    }

    #[test]
    fn a_double_puts_the_track_of_the_deck_beside_where_it_is() {
        let mut bench = Bench::new();
        bench.press("library.double1", 1.0).unwrap();
        assert_eq!(loaded(&bench.state), [false, false], "nothing to double");
        playing(&mut bench);
        bench.sent();
        bench.press("library.double2", 1.0).unwrap();
        let load_id = |deck: usize| lock(&bench.state.tracks)[deck].as_ref().map(|t: &LoadedTrack| t.load_id);
        assert!(load_id(1).is_some() && load_id(1) != load_id(0), "a load of its own");
        assert!(bench.sent().iter().any(|c| matches!(c, Command::TakePlace { deck: 1, from: 0 })));
    }

    #[test]
    fn a_smart_pad_stops_its_sound_plays_it_or_is_sent_as_a_load() {
        let bench = Bench::new();
        let smart: Target = "deck1.sample3.smart".parse().unwrap();
        let load: Target = "deck1.sample3.load".parse().unwrap();
        assert_eq!(bench.actions.sent_as(&bench.state, &smart), Some(load));
        bench.press("deck1.sample3.smart", 1.0).unwrap();
        assert!(bench.sent().is_empty(), "the slot emptied after it was sent: nothing opened");
        let audio = std::sync::Arc::new(prisme_media::TrackAudio::from_interleaved(48_000, vec![0.0; 4_800]));
        crate::sampler::put(&bench.state, 0, 2, "kick.wav".into(), audio).unwrap();
        bench.run(0.01);
        assert_eq!(bench.actions.sent_as(&bench.state, &smart), None);
        bench.press("deck1.sample3.smart", 1.0).unwrap();
        bench.run(0.01);
        assert_eq!(bench.status(0).samples_playing, 1 << 2);
        bench.press("deck1.sample3.smart", 1.0).unwrap();
        assert!(matches!(bench.sent()[..], [Command::StopSample { deck: 0, slot: 2 }]));
    }
}
