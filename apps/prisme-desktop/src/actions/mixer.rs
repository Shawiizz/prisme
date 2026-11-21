//! The software mixer: the decks' channels and what they are mixed into, set by the actions
//! and the interface's commands alike. What the DJ sets is kept as the audio stops, and sent to
//! the engine once it runs.

use super::take_over;
use crate::auto_dj::{AutoDj, Takeover};
use crate::dto::{CrossfaderCurveDto, MixModeDto};
use crate::error::AppResult;
use crate::state::{lock, AppState};
use prisme_actions::switch::LiveSource;
use prisme_actions::{engine_band, engine_live, engine_side};
use prisme_control::{Band, Side};
use prisme_engine::Command;
use prisme_library::CueSlot;

/// External or software mixer, kept for the next start: the mode in use follows (an external
/// mixer needs an output pair per deck). Applied with the audio settings.
pub fn set_mix_mode(state: &AppState, mode: MixModeDto) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.mode = mode);
    route(state)
}

/// Applies the routing the preferences give to the running audio, if any.
pub fn route(state: &AppState) -> AppResult<()> {
    let Some(info) = lock(&state.session).as_ref().map(|s| s.info.clone()) else { return Ok(()) };
    state.send(Command::SetRouting { routing: state.routing(&info) })
}

/// A channel fader this low is closed: fader start stops its deck there.
const FADER_CLOSED: f32 = 1e-3;

/// The channel fader of `deck` (a linear gain). With fader start on the software mixer, a
/// fader raised from closed plays its deck, and closed stops it on its cue point.
pub fn set_volume(state: &AppState, deck: usize, gain: f32) -> AppResult<()> {
    AppState::deck_exists(deck)?;
    let (before, fader_start) = state
        .decks
        .change(|decks| (std::mem::replace(&mut decks[deck].volume, gain), decks[deck].fader_start));
    state.send_if_running(Command::SetVolume { deck, gain })?;
    let Some(play) = fader_start.then(|| opened(before, gain)).flatten() else { return Ok(()) };
    if !state.mixes_itself() {
        return Ok(());
    }
    let Ok(cue_s) = state.with_track(deck, |t| t.cue(CueSlot::Main).map_or(0.0, |c| c.position_s)) else {
        return Ok(());
    };
    state.send(Command::SetPlaying { deck, playing: play })?;
    if play {
        Ok(())
    } else {
        state.send(Command::Seek { deck, seconds: cue_s })
    }
}

/// A fader moved from `before` to `after`: `Some(true)` as it opens from closed, `Some(false)`
/// as it closes.
fn opened(before: f32, after: f32) -> Option<bool> {
    let open = after > FADER_CLOSED;
    (open != (before > FADER_CLOSED)).then_some(open)
}

/// The gain before the EQ (linear).
pub fn set_trim(state: &AppState, deck: usize, gain: f32) -> AppResult<()> {
    state.set_deck(deck, |d| d.strip.trim = gain, Command::SetTrim { deck, gain })
}

/// An EQ band's gain (linear, 1 = flat).
pub fn set_eq(state: &AppState, deck: usize, band: Band, gain: f32) -> AppResult<()> {
    state.set_deck(
        deck,
        |d| d.strip.eq[band as usize] = gain,
        Command::SetEq { deck, band: engine_band(band), gain },
    )
}

/// A band of the deck's EQ cut, or back.
pub fn set_eq_kill(state: &AppState, deck: usize, band: Band, kill: bool) -> AppResult<()> {
    let command = Command::SetEqKill { deck, band: engine_band(band), kill };
    state.set_deck(deck, |d| d.strip.kills[band as usize] = kill, command)
}

/// -1 = low-pass closed, 0 = off, 1 = high-pass closed.
pub fn set_filter(state: &AppState, deck: usize, position: f32) -> AppResult<()> {
    state.set_deck(deck, |d| d.strip.filter = position, Command::SetFilter { deck, position })
}

/// -1 full left, 1 full right; the Auto DJ lets the crossfader go.
pub fn set_crossfader(state: &AppState, auto_dj: Option<&AutoDj>, position: f32) -> AppResult<()> {
    take_over(auto_dj, Takeover::Crossfader);
    state.crossfader.set(position);
    state.send_if_running(Command::SetCrossfader { position })
}

/// Headphones: 0 = cued decks only, 1 = master only.
pub fn set_cue_mix(state: &AppState, mix: f32) -> AppResult<()> {
    state.set_mixer(|m| m.cue_mix = mix, Command::SetCueMix { mix })
}

/// -1 = the master on the left only, 1 = on the right only.
pub fn set_balance(state: &AppState, balance: f32) -> AppResult<()> {
    state.set_mixer(|m| m.balance = balance, Command::SetBalance { balance })
}

pub fn set_booth_gain(state: &AppState, gain: f32) -> AppResult<()> {
    state.set_mixer(|m| m.booth_gain = gain, Command::SetBoothGain { gain })
}

pub fn set_headphone_gain(state: &AppState, gain: f32) -> AppResult<()> {
    state.set_mixer(|m| m.headphone_gain = gain, Command::SetHeadphoneGain { gain })
}

/// The master's level (linear).
pub fn set_main_gain(state: &AppState, gain: f32) -> AppResult<()> {
    state.set_mixer(|m| m.main_gain = gain, Command::SetMasterGain { gain })
}

pub fn set_crossfader_curve(state: &AppState, curve: CrossfaderCurveDto) -> AppResult<()> {
    state.set_mixer(|m| m.crossfader_curve = curve, Command::SetCrossfaderCurve { curve: curve.into() })
}

/// The level of a live source (linear).
pub fn set_live_level(state: &AppState, source: LiveSource, gain: f32) -> AppResult<()> {
    let command = Command::SetLiveLevel { source: engine_live(source), gain };
    state.set_mixer(
        |m| match source {
            LiveSource::Mic => m.mic.level = gain,
            LiveSource::Aux => m.aux.level = gain,
        },
        command,
    )
}

/// The side of the crossfader `deck` is on.
pub fn set_crossfader_side(state: &AppState, deck: usize, side: Side) -> AppResult<()> {
    let command = Command::SetCrossfaderSide { deck, side: engine_side(side) };
    state.set_deck(deck, |d| d.side = Some(side), command)
}

/// Software mixer: the channel fader starts and stops the deck; kept for the next start.
pub fn set_fader_start(state: &AppState, deck: usize, enabled: bool) -> AppResult<()> {
    AppState::deck_exists(deck)?;
    lock(&state.decks)[deck].fader_start = enabled;
    state.update_preferences(|p| p.decks[deck].fader_start = enabled);
    Ok(())
}

/// Sends the deck to the headphones (software mixer).
pub fn set_deck_cue(state: &AppState, deck: usize, enabled: bool) -> AppResult<()> {
    state.set_deck(deck, |d| d.cue = enabled, Command::SetDeckCue { deck, enabled })
}

/// `deck` alone in the headphones, of the decks in use.
pub fn solo_cue(state: &AppState, deck: usize) -> AppResult<()> {
    let decks = state.decks_in_use();
    (0..decks).try_for_each(|d| set_deck_cue(state, d, d == deck))
}

/// The crossfader's sides swapped, or back. Kept for the next start.
pub fn set_crossfader_reverse(state: &AppState, enabled: bool) -> AppResult<()> {
    state.set_mixer(|m| m.crossfader_reverse = enabled, Command::SetCrossfaderReverse { enabled })
}

/// The deck's channel silenced in the master, after its fader, or heard again.
pub fn set_mute(state: &AppState, deck: usize, enabled: bool) -> AppResult<()> {
    state.set_deck(deck, |d| d.mute = enabled, Command::SetMute { deck, muted: enabled })
}

/// Headphones: the cue on the left and the master on the right, each in mono, or blended.
/// Kept for the next start.
pub fn set_split_cue(state: &AppState, enabled: bool) -> AppResult<()> {
    state.set_mixer(|m| m.split_cue = enabled, Command::SetSplitCue { enabled })
}

/// Heard in the master, or not.
pub fn set_live_on(state: &AppState, source: LiveSource, on: bool) -> AppResult<()> {
    lock(&state.live)[source as usize].on = on;
    state.send_if_running(Command::SetLiveOn { source: engine_live(source), on })
}

/// Heard in the headphones, or not.
pub fn set_live_cue(state: &AppState, source: LiveSource, cue: bool) -> AppResult<()> {
    lock(&state.live)[source as usize].cue = cue;
    state.send_if_running(Command::SetLiveCue { source: engine_live(source), cue })
}

/// While the microphone is on, the music ducks; kept for the next start.
pub fn set_talkover(state: &AppState, enabled: bool) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.talkover = enabled);
    send_talkover(state)
}

/// Talkover as the DJ left it, to the engine.
pub fn send_talkover(state: &AppState) -> AppResult<()> {
    let (enabled, depth_db) = state.preferences.read(|p| (p.mixer.talkover, p.mixer.talkover_depth_db));
    state.send_if_running(Command::SetTalkover { enabled, depth_db })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fader_start_plays_as_the_fader_opens_and_stops_as_it_closes() {
        assert_eq!(opened(0.0, 0.2), Some(true));
        assert_eq!(opened(0.2, 0.8), None, "already open");
        assert_eq!(opened(0.8, 0.0), Some(false));
        assert_eq!(opened(0.0, 0.0), None);
        assert_eq!(opened(0.0005, 0.0), None, "a closed fader trembling");
    }
}
