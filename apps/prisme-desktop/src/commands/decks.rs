use crate::dto::VinylModeDto;
use crate::error::{AppError, AppResult};
use crate::state::{deck_commands, lock, AppState};
use prisme_engine::Command;
use tauri::State;

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
        let d = &mut p.decks[deck];
        (d.vinyl_enabled, d.vinyl_mode, d.lead_in_s) = (enabled, mode, lead_in_s);
    });
    for c in deck_commands(deck, &settings) {
        state.send_if_running(c)?;
    }
    Ok(())
}

/// Swaps the turntable's left and right channels (crossed cables), kept for the next start.
#[tauri::command]
pub fn set_vinyl_swap(state: State<'_, AppState>, deck: usize, swap: bool) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].swap_channels = swap;
    state.update_preferences(|p| p.decks[deck].swap_channels = swap);
    if let Some(session) = lock(&state.session).as_mut() {
        let decoder = session.engine_config.decoder(swap);
        session
            .controller
            .send(Command::ReplaceDecoder { deck, decoder })
            .map_err(|_| AppError::EngineBusy)?;
    }
    Ok(())
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
