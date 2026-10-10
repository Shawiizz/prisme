use crate::dto::{BandDto, CrossfaderCurveDto, DeckSettingsDto, MixModeDto, MixerStateDto};
use crate::error::AppResult;
use crate::sampler;
use crate::state::{lock, AppState};
use prisme_engine::Command;
use tauri::State;

/// Deck and mixer settings as the backend knows them (to rebuild the UI).
#[tauri::command]
pub fn mixer_state(state: State<'_, AppState>) -> MixerStateDto {
    let mixer = state.preferences.get().mixer;
    let output_channels = lock(&state.session).as_ref().map(|s| s.info.output_channels);
    let samplers = lock(&state.samplers);
    MixerStateDto {
        decks: lock(&state.decks)
            .iter()
            .zip(samplers.iter())
            .map(|(d, samples)| DeckSettingsDto {
                pitch: d.pitch,
                volume: d.volume,
                cue: d.cue,
                vinyl_enabled: d.vinyl_enabled,
                vinyl_mode: d.vinyl_mode,
                lead_in_s: d.lead_in_s,
                swap_channels: d.swap_channels,
                key_lock: d.key_lock,
                strip: d.strip,
                fx: d.fx,
                samples: samples.iter().map(|s| s.as_ref().map(|s| sampler::name(&s.path))).collect(),
                sampler_gain: d.sampler_gain,
            })
            .collect(),
        crossfader: *lock(&state.crossfader),
        mode: mixer.mode,
        mode_in_use: output_channels.map(|n| state.routing(n).mode.into()),
        cue_mix: mixer.cue_mix,
        headphone_gain: mixer.headphone_gain,
        crossfader_curve: mixer.crossfader_curve,
        limiter: mixer.limiter,
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

/// Software mixer: gain before the EQ (linear).
#[tauri::command]
pub fn set_trim(state: State<'_, AppState>, deck: usize, gain: f32) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].strip.trim = gain;
    state.send_if_running(Command::SetTrim { deck, gain })
}

/// Software mixer: EQ band gain (linear, 1 = flat).
#[tauri::command]
pub fn set_eq(state: State<'_, AppState>, deck: usize, band: BandDto, gain: f32) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].strip.eq[band as usize] = gain;
    state.send_if_running(Command::SetEq { deck, band: band.into(), gain })
}

#[tauri::command]
pub fn set_eq_kill(state: State<'_, AppState>, deck: usize, band: BandDto, kill: bool) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].strip.kills[band as usize] = kill;
    state.send_if_running(Command::SetEqKill { deck, band: band.into(), kill })
}

/// Software mixer: -1 = low-pass closed, 0 = off, 1 = high-pass open.
#[tauri::command]
pub fn set_filter(state: State<'_, AppState>, deck: usize, position: f32) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].strip.filter = position;
    state.send_if_running(Command::SetFilter { deck, position })
}

/// Kept for the next start.
#[tauri::command]
pub fn set_crossfader_curve(state: State<'_, AppState>, curve: CrossfaderCurveDto) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.crossfader_curve = curve);
    state.send_if_running(Command::SetCrossfaderCurve { curve: curve.into() })
}

/// Master limiter; kept for the next start.
#[tauri::command]
pub fn set_limiter(state: State<'_, AppState>, enabled: bool) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.limiter = enabled);
    state.send_if_running(Command::SetLimiter { enabled })
}
