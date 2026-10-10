use crate::dto::{DeckSettingsDto, MixModeDto, MixerStateDto};
use crate::error::AppResult;
use crate::state::{lock, AppState};
use prisme_engine::Command;
use tauri::State;

/// Deck and mixer settings as the backend knows them (to rebuild the UI).
#[tauri::command]
pub fn mixer_state(state: State<'_, AppState>) -> MixerStateDto {
    let mixer = state.preferences.get().mixer;
    let output_channels = lock(&state.session).as_ref().map(|s| s.info.output_channels);
    MixerStateDto {
        decks: lock(&state.decks)
            .iter()
            .map(|d| DeckSettingsDto {
                pitch: d.pitch,
                volume: d.volume,
                cue: d.cue,
                vinyl_enabled: d.vinyl_enabled,
                vinyl_mode: d.vinyl_mode,
                lead_in_s: d.lead_in_s,
            })
            .collect(),
        crossfader: *lock(&state.crossfader),
        mode: mixer.mode,
        mode_in_use: output_channels.map(|n| state.routing(n).mode.into()),
        cue_mix: mixer.cue_mix,
        headphone_gain: mixer.headphone_gain,
    }
}

/// External or software mixer, kept for the next start; returns the mode in use.
#[tauri::command]
pub fn set_mix_mode(state: State<'_, AppState>, mode: MixModeDto) -> AppResult<Option<MixModeDto>> {
    state.update_preferences(|p| p.mixer.mode = mode);
    let Some(output_channels) = lock(&state.session).as_ref().map(|s| s.info.output_channels) else {
        return Ok(None);
    };
    let routing = state.routing(output_channels);
    state.send(Command::SetRouting { routing })?;
    Ok(Some(routing.mode.into()))
}

#[tauri::command]
pub fn set_volume(state: State<'_, AppState>, deck: usize, gain: f32) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].volume = gain;
    state.send(Command::SetVolume { deck, gain })
}

#[tauri::command]
pub fn set_crossfader(state: State<'_, AppState>, position: f32) -> AppResult<()> {
    *lock(&state.crossfader) = position;
    state.send(Command::SetCrossfader { position })
}

/// Sends the deck to the headphones (software mixer).
#[tauri::command]
pub fn set_deck_cue(state: State<'_, AppState>, deck: usize, enabled: bool) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].cue = enabled;
    state.send(Command::SetDeckCue { deck, enabled })
}

/// Headphones: 0 = cued decks only, 1 = master only. Kept for the next start.
#[tauri::command]
pub fn set_cue_mix(state: State<'_, AppState>, mix: f32) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.cue_mix = mix);
    state.send_if_running(Command::SetCueMix { mix })
}

#[tauri::command]
pub fn set_headphone_gain(state: State<'_, AppState>, gain: f32) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.headphone_gain = gain);
    state.send_if_running(Command::SetHeadphoneGain { gain })
}
