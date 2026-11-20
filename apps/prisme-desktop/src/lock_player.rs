//! The player of Android's lock screen and notifications (`Player.kt`): the track of the deck
//! heard (`prisme_actions::player`), its cover and where it is, a button for each deck to play or
//! pause it. Told from the status's clock, whether the page shows or not; Android keeps the sound
//! going while a deck plays (`PlaybackService.kt`).

use prisme_actions::player::{Place, Shown};
use prisme_actions::view::View;
use prisme_engine::MAX_DECKS;
use serde::Serialize;

#[cfg(target_os = "android")]
pub use android::LockPlayer;

/// A deck with a track, as the player shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlayerDeck {
    pub deck: usize,
    pub title: String,
    pub artist: Option<String>,
    pub playing: bool,
}

/// What the player shows but where the deck shown is, which it moves on by itself.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Showing {
    pub decks: Vec<PlayerDeck>,
    pub shown: Option<usize>,
    /// The file of the deck shown, which its cover goes with.
    pub path: Option<String>,
    pub duration_s: f64,
    /// Its buttons play and pause the decks.
    pub controls: bool,
}

/// What the player is told (`PlayerState.parse`).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlayerState<'s> {
    #[serde(flatten)]
    showing: &'s Showing,
    position_s: f64,
    speed: f64,
}

/// A deck's track as the player names it.
#[derive(Debug, Clone, PartialEq)]
pub struct Named {
    pub title: String,
    pub artist: Option<String>,
    pub path: String,
}

/// What the player shows of `view`, `heard` the deck heard and those that go, `named` the decks'
/// tracks; and where the deck shown is.
pub fn showing(
    view: &View,
    heard: Shown,
    named: &[Option<Named>; MAX_DECKS],
    controls: bool,
) -> (Showing, Place) {
    let decks = (0..view.decks_in_use)
        .filter_map(|deck| {
            let track = named[deck].as_ref()?;
            let (title, artist) = (track.title.clone(), track.artist.clone());
            Some(PlayerDeck { deck, title, artist, playing: heard.moves(deck) })
        })
        .collect();
    let shown = heard.deck.filter(|&deck| named[deck].is_some());
    let status = shown.map(|deck| &view.status.decks[deck]);
    let showing = Showing {
        decks,
        shown,
        path: shown.and_then(|deck| named[deck].as_ref()).map(|track| track.path.clone()),
        duration_s: status.map_or(0.0, |s| s.duration_s),
        controls,
    };
    let place = match (shown, status) {
        (Some(deck), Some(status)) => {
            Place { position_s: status.position_s, speed: if heard.moves(deck) { status.speed } else { 0.0 } }
        }
        _ => Place::default(),
    };
    (showing, place)
}

#[cfg(target_os = "android")]
mod android {
    use super::{showing, Named, PlayerState, Showing};
    use crate::actions::{Host, InTurn, Source};
    use crate::android_jni;
    use crate::folders;
    use crate::library_service::LibraryService;
    use crate::state::{lock, AppState};
    use jni::objects::{JObject, JValue};
    use jni::sys::jint;
    use jni::JNIEnv;
    use prisme_actions::input::Input;
    use prisme_actions::player::{Shown, Told};
    use prisme_actions::view::View;
    use prisme_control::{DeckAction, Target, MAX_DECKS};
    use std::path::Path;
    use std::sync::Arc;
    use tauri::{AppHandle, Manager};

    /// Where the player is shown from (`MainActivity.kt`).
    const ACTIVITY: &str = "com.shawiizz.prisme.MainActivity";

    /// Tells Android's player what it shows, when that changed.
    #[derive(Default)]
    pub struct LockPlayer {
        named: [Option<Named>; MAX_DECKS],
        controls: bool,
        told: Told<Showing>,
        cover: Option<String>,
    }

    impl LockPlayer {
        /// Reads the decks' tracks again, and whether the player's buttons play and pause them.
        pub fn read(&mut self, state: &AppState) {
            self.named = lock(&state.tracks).each_ref().map(|track| {
                track.as_ref().map(|t| Named {
                    title: t.tags.title.clone().unwrap_or_else(|| folders::title_from_path(&t.path)),
                    artist: t.tags.artist.clone(),
                    path: t.path.display().to_string(),
                })
            });
            self.controls = state.preferences.read(|p| p.dj.lock_controls);
        }

        /// What the player shows of `view` at `now_s`, `heard` the deck heard: its cover told when
        /// another track is shown, the rest when it changed or drifted (see [`Told::tell`]).
        pub fn follow(&mut self, app: &AppHandle, view: &View, heard: Shown, now_s: f64) {
            let (showing, place) = showing(view, heard, &self.named, self.controls);
            if showing.path != self.cover {
                self.cover = showing.path.clone();
                if let Some(path) = self.cover.clone() {
                    cover(app, path);
                }
            }
            if self.told.tell(&showing, place, now_s) {
                let state =
                    PlayerState { showing: &showing, position_s: place.position_s, speed: place.speed };
                update(&serde_json::to_string(&state).unwrap_or_default());
            }
        }
    }

    fn update(state: &str) {
        android_jni::call("show the player", |env, context| {
            let activity = android_jni::class(env, context, ACTIVITY)?;
            let state = env.new_string(state)?;
            let signature = "(Landroid/content/Context;Ljava/lang/String;)V";
            env.call_static_method(
                &activity,
                "showPlayer",
                signature,
                &[JValue::Object(context), JValue::Object(&state)],
            )?;
            Ok(())
        });
    }

    /// Reads the cover of `path` off the clock's thread, and gives it to the player: none, the
    /// logo of Prisme instead.
    fn cover(app: &AppHandle, path: String) {
        let library = app.state::<Arc<LibraryService>>().inner().clone();
        tauri::async_runtime::spawn_blocking(move || {
            let jpeg = library.db.cover(Path::new(&path)).ok().flatten();
            android_jni::call("show the cover", |env, context| {
                let activity = android_jni::class(env, context, ACTIVITY)?;
                let path = env.new_string(&path)?;
                let jpeg: JObject = match &jpeg {
                    Some(jpeg) => env.byte_array_from_slice(jpeg)?.into(),
                    None => JObject::null(),
                };
                let signature = "(Landroid/content/Context;Ljava/lang/String;[B)V";
                let args = [JValue::Object(context), JValue::Object(&path), JValue::Object(&jpeg)];
                env.call_static_method(&activity, "showCover", signature, &args)?;
                Ok(())
            });
        });
    }

    /// `Player.togglePlay`: a button of the player played or paused `deck`, as its PLAY key does.
    #[no_mangle]
    pub extern "system" fn Java_com_shawiizz_prisme_Player_togglePlay(
        _env: JNIEnv,
        _player: JObject,
        deck: jint,
    ) {
        let Some(app) = android_jni::app() else { return };
        let Some(deck) = usize::try_from(deck).ok().filter(|&deck| deck < MAX_DECKS) else { return };
        let play = Target::Deck(deck, DeckAction::Play);
        let mut turn = InTurn::default();
        for value in [1.0, 0.0] {
            app.perform(&play, Input::Value(value), Source::Interface { confirmed: false }, &mut turn);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::view_with;
    use crate::state::AppState;
    use prisme_session::JsonFile;

    fn named(title: &str) -> Option<Named> {
        Some(Named { title: title.into(), artist: None, path: format!("{title}.mp3") })
    }

    #[test]
    fn the_player_shows_the_deck_heard_with_its_track_and_a_button_for_each_deck_loaded() {
        let state = AppState::new(JsonFile::in_memory(Default::default()));
        let mut status = prisme_engine::EngineStatus { deck_count: 2, ..Default::default() };
        (status.decks[0].position_s, status.decks[0].duration_s, status.decks[0].speed) = (12.0, 300.0, 1.02);
        let view = view_with(&state, status, None, false, &Default::default(), 0.0);
        let tracks = [named("One"), None, None, None];
        let (playing, place) = showing(&view, Shown { moving: 0b01, deck: Some(0) }, &tracks, true);
        assert_eq!(
            (playing.shown, playing.path.as_deref(), playing.duration_s),
            (Some(0), Some("One.mp3"), 300.0)
        );
        assert_eq!(playing.decks, [PlayerDeck { deck: 0, title: "One".into(), artist: None, playing: true }]);
        assert_eq!(place, Place { position_s: 12.0, speed: 1.02 });
        let (stopped, place) = showing(&view, Shown { moving: 0, deck: Some(0) }, &tracks, true);
        assert_eq!((stopped.decks[0].playing, place.speed), (false, 0.0), "it stands");
        let (none, _) = showing(&view, Shown { moving: 0, deck: Some(1) }, &tracks, false);
        assert_eq!((none.shown, none.path), (None, None), "no track on the deck heard");
    }

    #[test]
    fn the_player_is_told_as_android_parses_it() {
        let showing = Showing { shown: Some(1), path: Some("a.mp3".into()), ..Default::default() };
        let state = PlayerState { showing: &showing, position_s: 3.0, speed: 1.0 };
        let told: serde_json::Value = serde_json::to_value(&state).unwrap();
        assert_eq!(
            told,
            serde_json::json!({
                "decks": [], "shown": 1, "path": "a.mp3", "durationS": 0.0, "controls": false,
                "positionS": 3.0, "speed": 1.0
            })
        );
    }
}
