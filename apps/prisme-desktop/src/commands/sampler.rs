//! A deck's sampler (see `sampler`).

use crate::error::{AppError, AppResult};
use crate::sampler;
use crate::state::{lock, AppState};
use prisme_engine::{Command, SAMPLE_SLOTS};
use std::path::PathBuf;
use tauri::State;

fn check_slot(deck: usize, slot: usize) -> AppResult<()> {
    AppState::check_deck(deck)?;
    if slot < SAMPLE_SLOTS {
        Ok(())
    } else {
        Err(AppError::Other(format!("no sampler slot {}", slot + 1)))
    }
}

/// Puts a sound in a slot; returns its name. Kept for the next start.
#[tauri::command]
pub async fn load_sample(
    state: State<'_, AppState>,
    deck: usize,
    slot: usize,
    path: String,
) -> AppResult<String> {
    check_slot(deck, slot)?;
    let file = PathBuf::from(&path);
    let audio = {
        let file = file.clone();
        tauri::async_runtime::spawn_blocking(move || sampler::decode(&file))
            .await
            .map_err(|e| AppError::Other(e.to_string()))??
    };
    sampler::put(&state, deck, slot, file.clone(), audio)?;
    state.update_preferences(|p| {
        let samples = &mut p.decks[deck].samples;
        samples.resize(SAMPLE_SLOTS, None);
        samples[slot] = Some(path);
    });
    Ok(sampler::name(&file))
}

#[tauri::command]
pub fn clear_sample(state: State<'_, AppState>, deck: usize, slot: usize) -> AppResult<()> {
    check_slot(deck, slot)?;
    lock(&state.samplers)[deck][slot] = None;
    state.update_preferences(|p| {
        let samples = &mut p.decks[deck].samples;
        if let Some(saved) = samples.get_mut(slot) {
            *saved = None;
        }
        while samples.last() == Some(&None) {
            samples.pop();
        }
    });
    state.send_if_running(Command::SetSample { deck, slot, sample: None })
}

/// Plays a slot from its start (on the next beat with quantize on).
#[tauri::command]
pub fn trigger_sample(state: State<'_, AppState>, deck: usize, slot: usize) -> AppResult<()> {
    check_slot(deck, slot)?;
    state.send(Command::TriggerSample { deck, slot })
}

#[tauri::command]
pub fn stop_sample(state: State<'_, AppState>, deck: usize, slot: usize) -> AppResult<()> {
    check_slot(deck, slot)?;
    state.send(Command::StopSample { deck, slot })
}

#[tauri::command]
pub fn set_sampler_gain(state: State<'_, AppState>, deck: usize, gain: f32) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].sampler_gain = gain;
    state.send_if_running(Command::SetSamplerGain { deck, gain })
}
