use crate::dto::{DeckSettingsDto, MixerStateDto, VinylModeDto};
use crate::error::AppResult;
use crate::preferences::DeckPreferences;
use crate::state::{deck_commands, lock, AppState};
use prisme_engine::Command;
use tauri::State;

/// Deck settings and crossfader as the backend knows them (to rebuild the UI).
#[tauri::command]
pub fn mixer_state(state: State<'_, AppState>) -> MixerStateDto {
    MixerStateDto {
        decks: lock(&state.decks)
            .iter()
            .map(|d| DeckSettingsDto {
                pitch: d.pitch,
                volume: d.volume,
                vinyl_enabled: d.vinyl_enabled,
                vinyl_mode: d.vinyl_mode,
                lead_in_s: d.lead_in_s,
            })
            .collect(),
        crossfader: *lock(&state.crossfader),
    }
}

#[tauri::command]
pub fn set_playing(state: State<'_, AppState>, deck: usize, playing: bool) -> AppResult<()> {
    AppState::check_deck(deck)?;
    state.send(Command::SetPlaying { deck, playing })
}

#[tauri::command]
pub fn seek(state: State<'_, AppState>, deck: usize, seconds: f64) -> AppResult<()> {
    AppState::check_deck(deck)?;
    state.send(Command::Seek { deck, seconds })
}

#[tauri::command]
pub fn set_pitch(state: State<'_, AppState>, deck: usize, ratio: f64) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].pitch = ratio;
    state.send(Command::SetPitch { deck, ratio })
}

#[tauri::command]
pub fn set_volume(state: State<'_, AppState>, deck: usize, gain: f32) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].volume = gain;
    state.send(Command::SetVolume { deck, gain })
}

/// Vinyl control of a deck, kept for the next start.
#[tauri::command]
pub fn set_vinyl(
    state: State<'_, AppState>,
    deck: usize,
    enabled: bool,
    mode: VinylModeDto,
    lead_in_s: f64,
) -> AppResult<()> {
    AppState::check_deck(deck)?;
    let settings = {
        let mut decks = lock(&state.decks);
        let d = &mut decks[deck];
        d.vinyl_enabled = enabled;
        d.vinyl_mode = mode;
        d.lead_in_s = lead_in_s;
        *d
    };
    state.update_preferences(|p| {
        p.decks[deck] = DeckPreferences { vinyl_enabled: enabled, vinyl_mode: mode, lead_in_s };
    });
    for c in deck_commands(deck, &settings) {
        state.send_if_running(c)?;
    }
    Ok(())
}

#[tauri::command]
pub fn set_crossfader(state: State<'_, AppState>, position: f32) -> AppResult<()> {
    *lock(&state.crossfader) = position;
    state.send(Command::SetCrossfader { position })
}

#[tauri::command]
pub fn set_sync_locked(state: State<'_, AppState>, deck: usize, locked: bool) -> AppResult<()> {
    AppState::check_deck(deck)?;
    state.send(Command::SetSyncLocked { deck, locked })
}

#[tauri::command]
pub fn sync_once(state: State<'_, AppState>, deck: usize) -> AppResult<()> {
    AppState::check_deck(deck)?;
    state.send(Command::SyncOnce { deck })
}

/// `None`: the leader is picked automatically.
#[tauri::command]
pub fn set_sync_leader(state: State<'_, AppState>, deck: Option<usize>) -> AppResult<()> {
    if let Some(d) = deck {
        AppState::check_deck(d)?;
    }
    state.send(Command::SetSyncLeader { deck })
}
