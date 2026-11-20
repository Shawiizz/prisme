//! The software mixer: the decks' channels and what they are mixed into, set by the actions
//! and the interface's commands alike. What the DJ sets is kept as the audio stops, and sent to
//! the engine once it runs.

use super::take_over;
use crate::auto_dj::{AutoDj, Takeover};
use crate::dto::{BandDto, CrossfaderCurveDto, LiveSourceDto};
use crate::error::AppResult;
use crate::state::{lock, AppState};
use prisme_engine::Command;
use prisme_library::CueSlot;

/// A channel fader this low is closed: fader start stops its deck there.
const FADER_CLOSED: f32 = 1e-3;

/// The channel fader of `deck` (a linear gain). With fader start on the software mixer, a
/// fader raised from closed plays its deck, and closed stops it on its cue point.
pub fn set_volume(state: &AppState, deck: usize, gain: f32) -> AppResult<()> {
    AppState::check_deck(deck)?;
    let (before, fader_start) = {
        let mut decks = lock(&state.decks);
        (std::mem::replace(&mut decks[deck].volume, gain), decks[deck].fader_start)
    };
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
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].strip.trim = gain;
    state.send_if_running(Command::SetTrim { deck, gain })
}

/// An EQ band's gain (linear, 1 = flat).
pub fn set_eq(state: &AppState, deck: usize, band: BandDto, gain: f32) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].strip.eq[band as usize] = gain;
    state.send_if_running(Command::SetEq { deck, band: band.into(), gain })
}

/// A band of the deck's EQ cut, or back.
pub fn set_eq_kill(state: &AppState, deck: usize, band: BandDto, kill: bool) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].strip.kills[band as usize] = kill;
    state.send_if_running(Command::SetEqKill { deck, band: band.into(), kill })
}

/// -1 = low-pass closed, 0 = off, 1 = high-pass closed.
pub fn set_filter(state: &AppState, deck: usize, position: f32) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].strip.filter = position;
    state.send_if_running(Command::SetFilter { deck, position })
}

/// -1 full left, 1 full right; the Auto DJ lets the crossfader go.
pub fn set_crossfader(state: &AppState, auto_dj: Option<&AutoDj>, position: f32) -> AppResult<()> {
    take_over(auto_dj, Takeover::Crossfader);
    *lock(&state.crossfader) = position;
    state.send_if_running(Command::SetCrossfader { position })
}

/// Headphones: 0 = cued decks only, 1 = master only.
pub fn set_cue_mix(state: &AppState, mix: f32) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.cue_mix = mix);
    state.send_if_running(Command::SetCueMix { mix })
}

/// -1 = the master on the left only, 1 = on the right only.
pub fn set_balance(state: &AppState, balance: f32) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.balance = balance);
    state.send_if_running(Command::SetBalance { balance })
}

pub fn set_booth_gain(state: &AppState, gain: f32) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.booth_gain = gain);
    state.send_if_running(Command::SetBoothGain { gain })
}

pub fn set_headphone_gain(state: &AppState, gain: f32) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.headphone_gain = gain);
    state.send_if_running(Command::SetHeadphoneGain { gain })
}

/// The master's level (linear).
pub fn set_main_gain(state: &AppState, gain: f32) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.main_gain = gain);
    state.send_if_running(Command::SetMasterGain { gain })
}

pub fn set_crossfader_curve(state: &AppState, curve: CrossfaderCurveDto) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.crossfader_curve = curve);
    state.send_if_running(Command::SetCrossfaderCurve { curve: curve.into() })
}

/// The level of a live source (linear).
pub fn set_live_level(state: &AppState, source: LiveSourceDto, gain: f32) -> AppResult<()> {
    state.update_preferences(|p| match source {
        LiveSourceDto::Mic => p.mixer.mic.level = gain,
        LiveSourceDto::Aux => p.mixer.aux.level = gain,
    });
    state.send_if_running(Command::SetLiveLevel { source: source.into(), gain })
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
