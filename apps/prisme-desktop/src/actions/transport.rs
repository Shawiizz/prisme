//! A deck's transport and loops as the engine plays them: what the controllers' keys and the
//! interface's buttons both send.

use super::take_over;
use crate::auto_dj::{AutoDj, Takeover};
use crate::error::AppResult;
use crate::state::AppState;
use prisme_engine::{Command, LoopRegion};

/// The speed change while a bend key is held, and while a small one is.
pub const PITCH_BEND: f64 = 0.03;
pub const SMALL_BEND: f64 = 0.01;
/// A step of the tempo (a ratio), kept.
pub const TEMPO_STEP: f64 = 0.001;
/// The length of the loop a deck's loop key plays (beats), whatever its size.
pub const LOOP_KEY_BEATS: f64 = 4.0;
/// Seconds of track a second a search runs through beside the track's play.
pub const SEARCH_SPEED: f64 = 8.0;

/// Sends `command` to the engine for `deck`, one of those in use.
pub fn send(state: &AppState, deck: usize, command: Command) -> AppResult<()> {
    state.deck_in_use(deck)?;
    state.send(command)
}

/// Sends `command` to the engine for `deck`, one of those in use, moved by hand: the Auto DJ
/// lets the deck go.
pub fn send_by_hand(
    state: &AppState,
    auto_dj: Option<&AutoDj>,
    deck: usize,
    command: Command,
) -> AppResult<()> {
    state.deck_in_use(deck)?;
    take_over(auto_dj, Takeover::Transport(deck));
    state.send(command)
}

/// So many beats ahead, or back below zero; the Auto DJ lets the deck go.
pub fn beat_jump(state: &AppState, auto_dj: Option<&AutoDj>, deck: usize, beats: f64) -> AppResult<()> {
    send_by_hand(state, auto_dj, deck, Command::BeatJump { deck, beats })
}

/// Stores the loop of `deck` from `start_s` to `end_s`, playing it if `active`; without them,
/// forgets it.
pub fn set_loop(
    state: &AppState,
    deck: usize,
    start_s: Option<f64>,
    end_s: Option<f64>,
    active: bool,
) -> AppResult<()> {
    let region = start_s.zip(end_s).filter(|(s, e)| s.is_finite() && e > s);
    let region = region.map(|(start_s, end_s)| LoopRegion { start_s, end_s });
    send(state, deck, Command::SetLoop { deck, region, active: active && region.is_some() })
}

/// The track on `deck` played or paused; the Auto DJ lets the deck go.
pub fn set_playing(state: &AppState, auto_dj: Option<&AutoDj>, deck: usize, playing: bool) -> AppResult<()> {
    send_by_hand(state, auto_dj, deck, Command::SetPlaying { deck, playing })
}

/// To `seconds` into the track; the Auto DJ lets the deck go.
pub fn seek(state: &AppState, auto_dj: Option<&AutoDj>, deck: usize, seconds: f64) -> AppResult<()> {
    send_by_hand(state, auto_dj, deck, Command::Seek { deck, seconds })
}

/// To `seconds` into the track, within it; nothing without a track.
pub fn seek_within(state: &AppState, auto_dj: Option<&AutoDj>, deck: usize, seconds: f64) -> AppResult<()> {
    let duration_s = state.deck_status(deck)?.duration_s;
    if duration_s <= 0.0 {
        return Ok(());
    }
    seek(state, auto_dj, deck, seconds.clamp(0.0, duration_s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::bench::Bench;

    #[test]
    fn a_seek_within_stays_in_the_track_and_does_nothing_without_one() {
        let bench = Bench::new();
        bench.load(0, 10.0, None);
        for (asked, sent) in [(4.0, 4.0), (25.0, 10.0), (-3.0, 0.0)] {
            seek_within(&bench.state, None, 0, asked).unwrap();
            assert!(
                matches!(bench.sent()[..], [Command::Seek { deck: 0, seconds }] if seconds == sent),
                "{asked}"
            );
        }
        seek_within(&bench.state, None, 1, 4.0).unwrap();
        assert!(bench.sent().is_empty(), "no track on the second deck");
        assert!(seek_within(&bench.state, None, 7, 4.0).is_err(), "no such deck");
    }
}
