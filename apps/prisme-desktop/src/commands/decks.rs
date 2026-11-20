use crate::actions::deck;
use crate::audio_host::layer_partner;
use crate::auto_dj::{AutoDj, Takeover};
use crate::commands::audio::start_audio;
use crate::commands::blocking;
use crate::dto::VinylChangeDto;
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState};
use prisme_engine::Command;
use prisme_timecode::TimecodeFormat;
use tauri::{AppHandle, Manager, State};

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

/// A setting of the vinyl control of `deck` changed alone (see `deck::set_vinyl`); it is put on
/// and off as an action (`deckN.vinyl`), which no change given here can be.
#[tauri::command]
pub fn set_vinyl(state: State<'_, AppState>, deck: usize, change: VinylChangeDto) -> AppResult<()> {
    deck::set_vinyl(&state, deck, change)
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

/// The range of the tempo fader of `deck` (0.08 = ±8 %), or of every deck (`None`), which
/// then all follow the general one; kept for the next start.
#[tauri::command]
pub fn set_tempo_range(state: State<'_, AppState>, deck: Option<usize>, range: f64) -> AppResult<()> {
    deck::set_tempo_range(&state, deck, range)
}
