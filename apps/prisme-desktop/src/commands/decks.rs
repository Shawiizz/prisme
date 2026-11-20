use crate::actions::deck;
use crate::audio_host::layer_partner;
use crate::auto_dj::{AutoDj, Takeover};
use crate::commands::audio::start_audio;
use crate::commands::blocking;
use crate::dto::{RpmDto, VinylModeDto};
use crate::error::{AppError, AppResult};
use crate::preferences::DeckPreferences;
use crate::state::{deck_commands, lock, AppState};
use prisme_engine::Command;
use prisme_timecode::TimecodeFormat;
use tauri::{AppHandle, Manager, State};

const MAX_KEY_SHIFT: u8 = 12;

#[tauri::command]
pub fn set_playing(
    state: State<'_, AppState>,
    auto_dj: State<'_, AutoDj>,
    deck: usize,
    playing: bool,
) -> AppResult<()> {
    state.has_deck(deck)?;
    auto_dj.take_over(Takeover::Transport(deck));
    state.send(Command::SetPlaying { deck, playing })
}

#[tauri::command]
pub fn seek(
    state: State<'_, AppState>,
    auto_dj: State<'_, AutoDj>,
    deck: usize,
    seconds: f64,
) -> AppResult<()> {
    state.has_deck(deck)?;
    auto_dj.take_over(Takeover::Transport(deck));
    state.send(Command::Seek { deck, seconds })
}

/// The track dragged by hand to `seconds` on the scrolling waveform at `at_ms` on the
/// pointer's clock, or let go (`None`).
#[tauri::command]
pub fn hand(
    state: State<'_, AppState>,
    auto_dj: State<'_, AutoDj>,
    deck: usize,
    seconds: Option<f64>,
    at_ms: f64,
) -> AppResult<()> {
    state.has_deck(deck)?;
    auto_dj.take_over(Takeover::Transport(deck));
    state.send(Command::Hand { deck, position_s: seconds, at_s: at_ms / 1000.0 })
}

/// The edge of a jog wheel turned by `turns` (positive forwards) at `at_ms`: a playing track
/// pushed or held back for a moment, a stopped one moved.
#[tauri::command]
pub fn jog(
    state: State<'_, AppState>,
    auto_dj: State<'_, AutoDj>,
    deck: usize,
    turns: f64,
    at_ms: f64,
) -> AppResult<()> {
    state.has_deck(deck)?;
    auto_dj.take_over(Takeover::Transport(deck));
    state.send(Command::Jog { deck, turns, at_s: at_ms / 1000.0 })
}

/// Whether a touch of the controller's jog wheel of `deck` holds the record (else the wheel
/// only nudges it).
#[tauri::command]
pub fn set_jog_scratch(state: State<'_, AppState>, deck: usize, on: bool) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].jog_scratch = on;
    Ok(())
}

#[tauri::command]
pub fn set_pitch(state: State<'_, AppState>, deck: usize, ratio: f64) -> AppResult<()> {
    deck::set_pitch(&state, deck, ratio)
}

/// Vinyl control of a deck, kept for the next start. Returns the deck it took the turntable
/// from, if they are layered on it (see `audio_host::deck_pair`). A deck a record lets go of
/// plays on at the record's tempo.
#[tauri::command]
pub fn set_vinyl(
    state: State<'_, AppState>,
    deck: usize,
    enabled: bool,
    mode: VinylModeDto,
    rpm: RpmDto,
    lead_in_s: Option<f64>,
) -> AppResult<Option<usize>> {
    AppState::check_deck(deck)?;
    if !enabled {
        state.let_go_of_record(deck);
    }
    let settings = {
        let mut decks = lock(&state.decks);
        let d = &mut decks[deck];
        d.vinyl_enabled = enabled;
        d.vinyl_mode = mode;
        d.rpm = rpm;
        d.lead_in_s = lead_in_s;
        *d
    };
    state.update_preferences(|p| {
        let d = &mut p.decks[deck];
        (d.vinyl_enabled, d.vinyl_mode, d.rpm, d.lead_in_s) = (enabled, mode, rpm, lead_in_s);
    });
    let freed = enabled.then(|| state.take_turntable(deck)).flatten();
    for c in deck_commands(deck, &settings) {
        state.send_if_running(c)?;
    }
    Ok(freed)
}

/// Swaps the turntable's left and right channels (crossed cables), kept for the next start.
#[tauri::command]
pub fn set_vinyl_swap(state: State<'_, AppState>, deck: usize, swap: bool) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].swap_channels = swap;
    state.update_preferences(|p| p.decks[deck].swap_channels = swap);
    replace_decoder(&state, deck)
}

/// The record on the deck's turntable (a timecode format's id), kept for the next start: on
/// the deck layered with it too, which reads the same turntable. Returns the decks given it.
/// The simulated turntable plays deck A's: the audio starts again with the new one.
#[tauri::command]
pub async fn set_timecode(app: AppHandle, deck: usize, format: String) -> AppResult<Vec<usize>> {
    AppState::check_deck(deck)?;
    let record = TimecodeFormat::by_id(&format).ok_or(AppError::UnknownFormat(format))?;
    blocking(move || {
        let state = app.state::<AppState>();
        let (partner, simulated) = match lock(&state.session).as_ref() {
            Some(s) => (
                layer_partner(deck, s.info.decks, s.info.input_channels),
                s.info.simulated.then(|| s.settings.clone()),
            ),
            None => (None, None),
        };
        let decks: Vec<usize> = std::iter::once(deck).chain(partner).collect();
        for &d in &decks {
            lock(&state.decks)[d].timecode = record;
        }
        state.update_preferences(|p| {
            for &d in &decks {
                p.decks[d].timecode = Some(record.id.into());
            }
        });
        match simulated {
            Some(settings) if decks.contains(&0) => start_audio(&state, &settings).map(drop)?,
            _ => decks.iter().try_for_each(|&d| replace_decoder(&state, d))?,
        }
        Ok(decks)
    })
    .await
}

/// Gives the deck a decoder for its record and its channels.
fn replace_decoder(state: &AppState, deck: usize) -> AppResult<()> {
    let (record, swap) = {
        let d = lock(&state.decks)[deck];
        (d.timecode, d.swap_channels)
    };
    if let Some(session) = lock(&state.session).as_mut() {
        let decoder = session.engine_config.decoder(record, swap);
        session
            .controller
            .send(Command::ReplaceDecoder { deck, decoder })
            .map_err(|_| AppError::EngineBusy)?;
    }
    Ok(())
}

/// The deck plays its input as it comes (a record, a CD player) instead of its track.
#[tauri::command]
pub fn set_passthrough(state: State<'_, AppState>, deck: usize, enabled: bool) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].passthrough = enabled;
    state.send_if_running(Command::SetPassthrough { deck, enabled })
}

/// The range of the tempo fader of `deck` (0.08 = ±8 %), or of every deck (`None`), which
/// then all follow the general one; kept for the next start.
#[tauri::command]
pub fn set_tempo_range(state: State<'_, AppState>, deck: Option<usize>, range: f64) -> AppResult<()> {
    if !(range > 0.0 && range <= 1.0) {
        return Err(AppError::Other("a tempo range is within ±100 %".into()));
    }
    match deck {
        Some(deck) => {
            AppState::check_deck(deck)?;
            lock(&state.decks)[deck].pitch_range = Some(range);
            state.update_preferences(|p| p.decks[deck].pitch_range = Some(range));
        }
        None => {
            lock(&state.decks).iter_mut().for_each(|d| d.pitch_range = None);
            state.update_preferences(|p| {
                p.dj.pitch_range = range;
                p.decks.iter_mut().for_each(|d| d.pitch_range = None);
            });
        }
    }
    Ok(())
}

/// The deck's size in beats, what its jumps go by and its sized loop lasts; kept for the next
/// start.
#[tauri::command]
pub fn set_deck_size(state: State<'_, AppState>, deck: usize, beats: f64) -> AppResult<()> {
    AppState::check_deck(deck)?;
    if !DeckPreferences::SIZE_BEATS.contains(&beats) {
        return Err(AppError::Other("a deck's size is from 1/16 of a beat to 64 beats".into()));
    }
    lock(&state.decks)[deck].size_beats = beats;
    state.update_preferences(|p| p.decks[deck].size_beats = beats);
    Ok(())
}

/// At the end, the track goes on from its start.
#[tauri::command]
pub fn set_repeat(state: State<'_, AppState>, deck: usize, enabled: bool) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].repeat = enabled;
    state.send_if_running(Command::SetRepeat { deck, enabled })
}

/// The pitch stays when the speed changes; kept for the next start.
#[tauri::command]
pub fn set_key_lock(state: State<'_, AppState>, deck: usize, enabled: bool) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].key_lock = enabled;
    state.update_preferences(|p| p.decks[deck].key_lock = enabled);
    state.send_if_running(Command::SetKeyLock { deck, enabled })
}

/// Transposes the track on `deck` until the next load.
#[tauri::command]
pub fn set_key_shift(state: State<'_, AppState>, deck: usize, semitones: i8) -> AppResult<()> {
    if semitones.unsigned_abs() > MAX_KEY_SHIFT {
        return Err(AppError::Other(format!("a key shift is ±{MAX_KEY_SHIFT} semitones at most")));
    }
    state.with_track(deck, |track| track.key_shift = semitones)?;
    state.send_if_running(Command::SetKeyShift { deck, semitones })
}

#[tauri::command]
pub fn set_sync_locked(state: State<'_, AppState>, deck: usize, locked: bool) -> AppResult<()> {
    state.has_deck(deck)?;
    state.send(Command::SetSyncLocked { deck, locked })
}

#[tauri::command]
pub fn sync_once(state: State<'_, AppState>, deck: usize) -> AppResult<()> {
    state.has_deck(deck)?;
    state.send(Command::SyncOnce { deck })
}

/// `None`: the leader is picked automatically.
#[tauri::command]
pub fn set_sync_leader(state: State<'_, AppState>, deck: Option<usize>) -> AppResult<()> {
    deck::set_sync_leader(&state, deck)
}
