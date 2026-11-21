use crate::actions::{deck, transport, Actions, Order};
use crate::audio_host::layer_partner;
use crate::auto_dj::{AutoDj, Takeover};
use crate::commands::audio::start_audio;
use crate::commands::blocking;
use crate::dto::VinylChangeDto;
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState};
use prisme_actions::screen::Drive;
use prisme_engine::Command;
use prisme_timecode::TimecodeFormat;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

/// The track dragged by hand to `seconds` on the scrolling waveform at `at_ms` on the
/// pointer's clock (not before its start), or let go (`None`).
#[tauri::command]
pub fn hand(
    state: State<'_, AppState>,
    auto_dj: State<'_, AutoDj>,
    deck: usize,
    seconds: Option<f64>,
    at_ms: f64,
) -> AppResult<()> {
    state.deck_in_use(deck)?;
    auto_dj.take_over(Takeover::Transport(deck));
    let position_s = seconds.map(|s| s.max(0.0));
    state.send(Command::Hand { deck, position_s, at_s: at_ms / 1000.0 })
}

/// The track on `deck` put at `seconds`, within it; nothing without a track.
#[tauri::command]
pub fn seek_deck(
    state: State<'_, AppState>,
    auto_dj: State<'_, AutoDj>,
    deck: usize,
    seconds: f64,
) -> AppResult<()> {
    transport::seek_within(&state, Some(&auto_dj), deck, seconds)
}

/// The edge of a jog wheel turned by `seconds` of track (positive forwards) at `at_ms`: a
/// playing track pushed or held back for a moment, a stopped one moved.
#[tauri::command]
pub fn jog(
    state: State<'_, AppState>,
    auto_dj: State<'_, AutoDj>,
    deck: usize,
    seconds: f64,
    at_ms: f64,
) -> AppResult<()> {
    state.deck_in_use(deck)?;
    auto_dj.take_over(Takeover::Transport(deck));
    let turns = seconds / prisme_engine::JOG_TURN_S;
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
    AppState::deck_exists(deck)?;
    lock(&state.decks)[deck].swap_channels = swap;
    state.update_preferences(|p| p.decks[deck].swap_channels = swap);
    replace_decoder(&state, deck)
}

/// Who drives `deck`: itself, its record or its input (see `deck::set_mode`).
#[tauri::command]
pub fn set_deck_mode(state: State<'_, AppState>, deck: usize, mode: Drive) -> AppResult<()> {
    deck::set_mode(&state, deck, mode)
}

/// Another record on the deck's turntable (`record`: the id of one of its sides), on the side the
/// one before was on if it has it (see `set_timecode`).
#[tauri::command]
pub async fn set_record(app: AppHandle, deck: usize, record: String) -> AppResult<Vec<usize>> {
    AppState::deck_exists(deck)?;
    let chosen = TimecodeFormat::by_id(&record).ok_or(AppError::UnknownFormat(record))?;
    let side = lock(&app.state::<AppState>().decks)[deck].timecode.side;
    set_timecode(app, deck, on_side(chosen, side).id.to_owned()).await
}

/// The side `side` of the record `chosen` is a side of, else `chosen`.
fn on_side(chosen: &'static TimecodeFormat, side: Option<&str>) -> &'static TimecodeFormat {
    let sides =
        prisme_timecode::FORMATS.iter().filter(|f| f.maker == chosen.maker && f.record == chosen.record);
    sides.into_iter().find(|f| f.side == side).unwrap_or(chosen)
}

/// The record on the deck's turntable (a timecode format's id), kept for the next start: on
/// the deck layered with it too, which reads the same turntable. Returns the decks given it.
/// The simulated turntable plays deck A's: the audio starts again with the new one.
#[tauri::command]
pub async fn set_timecode(app: AppHandle, deck: usize, format: String) -> AppResult<Vec<usize>> {
    AppState::deck_exists(deck)?;
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

/// Loads the track at `path` on `deck`, or on the first deck without a track (deck A if none):
/// a row of the library, a track dropped, one the command palette found; `confirmed`, the DJ said
/// yes to what it replaces. Sent in turn with the interface's actions (`order`).
#[tauri::command]
pub async fn load_track(
    app: AppHandle,
    path: String,
    deck: Option<usize>,
    confirmed: bool,
    order: Order,
) -> AppResult<()> {
    blocking(move || {
        let actions = app.state::<Actions>();
        actions.answered(order, |answer| {
            actions.load_track(&app, PathBuf::from(path), deck, confirmed, answer);
            Ok(())
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::TimecodeMakerDto;

    #[test]
    fn another_record_goes_on_the_side_the_one_before_was_on() {
        let by_id = |id: &str| TimecodeFormat::by_id(id).unwrap();
        assert_eq!(on_side(by_id("serato_cv02_a"), Some("B")).id, "serato_cv02_b");
        assert_eq!(on_side(by_id("serato_cv02_b"), None).id, "serato_cv02_b", "no side: the one chosen");
    }

    #[test]
    fn the_records_are_listed_by_maker_each_with_its_sides() {
        let makers = TimecodeMakerDto::of(prisme_timecode::FORMATS);
        let serato = makers.iter().find(|m| m.name == "Serato").unwrap();
        let cv02 = serato.records.iter().find(|r| r.name == "Control vinyl CV02").unwrap();
        assert_eq!(cv02.sides.iter().map(|s| s.side).collect::<Vec<_>>(), [Some("A"), Some("B")]);
        let formats: usize = makers.iter().flat_map(|m| &m.records).map(|r| r.sides.len()).sum();
        assert_eq!(formats, prisme_timecode::FORMATS.len());
    }
}
