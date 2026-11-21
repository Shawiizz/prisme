//! The live sources of the software mixer: a microphone and an aux line, and talkover.

use crate::actions::mixer;
use crate::choices;
use crate::dto::InputChannelsDto;
use crate::error::{AppError, AppResult};
use crate::preferences::MixerPreferences;
use crate::state::{live_preferences, lock, AppState};
use prisme_actions::switch::LiveSource;
use prisme_engine::Command;
use tauri::State;

/// The inputs `source` comes in on, kept for the next start: in use on the running device if
/// the turntables leave them free. Refused on an input the other source has.
#[tauri::command]
pub fn set_live_input(
    state: State<'_, AppState>,
    source: LiveSource,
    input: Option<InputChannelsDto>,
) -> AppResult<()> {
    choose_live_input(&state, source, input)?;
    let info = lock(&state.session).as_ref().map(|s| s.info.clone());
    let channels = state.live_input(source, info.as_ref());
    state.send_if_running(Command::SetLiveInput {
        source: prisme_actions::engine_live(source),
        channels: channels.map(Into::into),
    })
}

/// `source` kept on `input`, unless the other source has one of its inputs.
fn choose_live_input(state: &AppState, source: LiveSource, input: Option<InputChannelsDto>) -> AppResult<()> {
    let mut taken = false;
    state.update_preferences(|p| {
        let other = live_preferences(&p.mixer, source.other()).input;
        taken = input.zip(other).is_some_and(|(a, b)| choices::overlap(a, b));
        if !taken {
            match source {
                LiveSource::Mic => p.mixer.mic.input = input,
                LiveSource::Aux => p.mixer.aux.input = input,
            }
        }
    });
    if taken {
        return Err(AppError::InputTaken(source.other()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_microphone_and_the_aux_line_never_share_an_input() {
        let state = AppState::new(prisme_session::JsonFile::in_memory(Default::default()));
        let pair = |first| Some(InputChannelsDto { first, stereo: true });
        let one = |first| Some(InputChannelsDto { first, stereo: false });
        choose_live_input(&state, LiveSource::Aux, pair(2)).unwrap();
        let refused = choose_live_input(&state, LiveSource::Mic, one(3));
        assert!(matches!(refused, Err(AppError::InputTaken(LiveSource::Aux))));
        assert_eq!(state.preferences.read(|p| p.mixer.mic.input), None);
        choose_live_input(&state, LiveSource::Mic, one(4)).unwrap();
        assert_eq!(state.preferences.read(|p| p.mixer.mic.input), one(4));
    }
}

/// How far the music ducks under the microphone (dB), one of
/// [`MixerPreferences::TALKOVER_DEPTHS`]; kept for the next start.
#[tauri::command]
pub fn set_talkover_depth(state: State<'_, AppState>, db: f32) -> AppResult<()> {
    if !MixerPreferences::TALKOVER_DEPTHS.contains(&db) {
        return Err(AppError::Other(format!("no talkover of {db} dB")));
    }
    state.update_preferences(|p| p.mixer.talkover_depth_db = db);
    mixer::send_talkover(&state)
}
